//! An HTTP transport that records requests and responses.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpResponse;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::RequestBody;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::TransportError;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use futures_util::StreamExt;
use futures_util::stream;
use http::Method;
use http::StatusCode;
use url::Url;

/// Header names never recorded (case-insensitive).
pub const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
    "x-goog-api-key",
    "cookie",
    "set-cookie",
    "openai-organization",
    "openai-project",
];

/// Which request and response headers are recorded.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum HeaderFilter {
    /// Everything except [`SENSITIVE_HEADERS`].
    DenySensitive,
    /// Only the listed names (case-insensitive), sensitive names excluded.
    Allow(HashSet<String>),
}

impl HeaderFilter {
    /// Returns `true` when `name` is recorded.
    #[must_use]
    pub fn keeps(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        if SENSITIVE_HEADERS.contains(&lower.as_str()) {
            return false;
        }
        match self {
            Self::DenySensitive => true,
            Self::Allow(names) => names.contains(&lower),
        }
    }

    fn apply(&self, headers: &Headers) -> Headers {
        let mut kept = Headers::new();
        for (name, value) in headers.iter_str() {
            if self.keeps(name) {
                // Values came from a header map, so they are valid.
                let _ = kept.insert(name, &value);
            }
        }
        kept
    }
}

/// Response half of a recorded exchange, filled while the body is read.
#[derive(Debug, Clone, Default)]
pub struct RecordedResponse {
    /// Status code, once the response arrived.
    pub status: Option<StatusCode>,
    /// Recorded response headers.
    pub headers: Headers,
    /// Body chunks in arrival order.
    pub chunks: Vec<Bytes>,
    /// `true` once the body stream ended.
    pub complete: bool,
    /// Transport failure, if the request or the body failed.
    pub error: Option<String>,
}

impl RecordedResponse {
    /// The body chunks concatenated.
    #[must_use]
    pub fn body(&self) -> Bytes {
        let mut body = Vec::new();
        for chunk in &self.chunks {
            body.extend_from_slice(chunk);
        }
        Bytes::from(body)
    }

    /// The body as UTF-8 text (lossy).
    #[must_use]
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body()).into_owned()
    }
}

/// One recorded exchange.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// Request method.
    pub method: Method,
    /// Request URL.
    pub url: Url,
    /// Recorded request headers.
    pub headers: Headers,
    /// Content type of the body, if any.
    pub content_type: Option<String>,
    /// Encoded request body.
    pub body: Bytes,
    /// The response, filled lazily as the caller reads it.
    pub response: Arc<Mutex<RecordedResponse>>,
}

impl RecordedRequest {
    /// The request body as JSON.
    ///
    /// # Errors
    ///
    /// Returns the parse error when the body is not JSON.
    pub fn body_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// The request body as UTF-8 text (lossy).
    #[must_use]
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// A snapshot of the response recorded so far.
    #[must_use]
    pub fn response(&self) -> RecordedResponse {
        lock(&self.response).clone()
    }
}

/// Wraps a transport and records every exchange.
///
/// Response bodies are recorded as the caller consumes them, so a stream
/// that is dropped early is recorded up to the last chunk read.
pub struct RecordingTransport {
    inner: SharedTransport,
    filter: HeaderFilter,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl fmt::Debug for RecordingTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingTransport")
            .field("filter", &self.filter)
            .field("requests", &lock(&self.requests).len())
            .finish_non_exhaustive()
    }
}

impl RecordingTransport {
    /// Records exchanges going through `inner`.
    #[must_use]
    pub fn new(inner: SharedTransport) -> Self {
        Self {
            inner,
            filter: HeaderFilter::DenySensitive,
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Records only the listed header names.
    #[must_use]
    pub fn with_header_allow_list<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.filter = HeaderFilter::Allow(
            names
                .into_iter()
                .map(|name| name.as_ref().to_ascii_lowercase())
                .collect(),
        );
        self
    }

    /// Recorded exchanges, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.requests).clone()
    }

    /// The most recent exchange.
    #[must_use]
    pub fn last_request(&self) -> Option<RecordedRequest> {
        lock(&self.requests).last().cloned()
    }

    /// Number of recorded exchanges.
    #[must_use]
    pub fn len(&self) -> usize {
        lock(&self.requests).len()
    }

    /// `true` when nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        lock(&self.requests).is_empty()
    }

    /// Forgets recorded exchanges.
    pub fn clear(&self) {
        lock(&self.requests).clear();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl HttpTransport for RecordingTransport {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let content_type = match &request.body {
            RequestBody::Empty => None,
            RequestBody::Bytes { content_type, .. } => Some(content_type.clone()),
            RequestBody::Multipart(_) => Some("multipart/form-data".to_owned()),
            _ => None,
        };
        let response = Arc::new(Mutex::new(RecordedResponse::default()));
        lock(&self.requests).push(RecordedRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: self.filter.apply(&request.headers),
            content_type,
            body: request.body.to_bytes(),
            response: Arc::clone(&response),
        });
        let filter = self.filter.clone();
        Box::pin(async move {
            match self.inner.execute(request).await {
                Ok(inner) => {
                    {
                        let mut recorded = lock(&response);
                        recorded.status = Some(inner.status);
                        recorded.headers = filter.apply(&inner.headers);
                    }
                    let body =
                        stream::unfold((inner.body, response), |(mut body, response)| async move {
                            match body.next().await {
                                Some(Ok(chunk)) => {
                                    lock(&response).chunks.push(chunk.clone());
                                    Some((Ok(chunk), (body, response)))
                                }
                                Some(Err(error)) => {
                                    lock(&response).error = Some(error.to_string());
                                    Some((Err(error), (body, response)))
                                }
                                None => {
                                    lock(&response).complete = true;
                                    None
                                }
                            }
                        });
                    Ok(HttpResponse::from_stream(
                        inner.status,
                        inner.headers,
                        Box::pin(body),
                    ))
                }
                Err(error) => {
                    lock(&response).error = Some(error.to_string());
                    Err(error)
                }
            }
        })
    }
}

/// Replaces API-key-like tokens (`sk-...`) and bearer tokens (`Bearer ...`)
/// in `text` with `[redacted]`.
#[must_use]
pub fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("sk-") {
            let end = stripped
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(stripped.len());
            if end >= 8 {
                out.push_str("[redacted]");
                rest = &stripped[end..];
                continue;
            }
        }
        if let Some(stripped) = rest.strip_prefix("Bearer ") {
            let end = stripped
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .unwrap_or(stripped.len());
            if end > 0 {
                out.push_str("Bearer [redacted]");
                rest = &stripped[end..];
                continue;
            }
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
            rest = chars.as_str();
        }
    }
    out
}

/// Returns `true` when `text` still contains a secret-like token.
#[must_use]
pub fn contains_secret(text: &str) -> bool {
    redact_secrets(text) != text
}
