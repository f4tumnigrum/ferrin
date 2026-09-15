//! Policy client for the OPA REST Data API.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::SharedTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::http::default_transport;
use ferrin_provider_util::http::read_body;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::secure_url::validate_url;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use http::Method;
use serde_json::json;
use url::Url;

use crate::client::PolicyClient;
use crate::error::PolicyError;
use crate::path::PolicyPath;

/// Default limit on the size of a decision response body.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

const USER_AGENT: &str = concat!("ferrin-policy/", env!("CARGO_PKG_VERSION"));
const BODY_EXCERPT_BYTES: usize = 1024;

/// Evaluates policies through a policy server implementing the OPA REST
/// Data API: `POST <base>/v1/data/<path>` with `{"input": ..}`, answering
/// `{"result": ..}`.
///
/// A missing `result` (an undefined rule) yields `JsonValue::Null`. The
/// server URL is validated with the configured [`UrlPolicy`] on every call;
/// the default policy requires HTTPS and a public host, so a local sidecar
/// needs `UrlPolicy::new().allow_http().allow_private_networks()`.
pub struct HttpPolicyClient {
    base_url: Url,
    headers: Headers,
    transport: SharedTransport,
    url_policy: UrlPolicy,
    timeout: Option<Duration>,
    max_response_bytes: u64,
}

impl fmt::Debug for HttpPolicyClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpPolicyClient")
            .field("base_url", &self.base_url.origin().ascii_serialization())
            .field("headers", &self.headers.masked())
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl HttpPolicyClient {
    /// Starts building a client for the server at `base_url`.
    #[must_use]
    pub fn builder(base_url: Url) -> HttpPolicyClientBuilder {
        HttpPolicyClientBuilder {
            base_url,
            headers: Headers::new(),
            transport: None,
            url_policy: UrlPolicy::new(),
            timeout: None,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }

    /// Creates a client with the default transport and URL policy.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Transport`] when the default transport cannot
    /// be created.
    pub fn new(base_url: Url) -> Result<Self, PolicyError> {
        Self::builder(base_url).build()
    }

    /// The server URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    fn data_url(&self, path: &PolicyPath) -> Result<Url, PolicyError> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|()| PolicyError::InvalidUrl {
                    message: "policy server url cannot be a base".to_owned(),
                })?;
            segments.pop_if_empty();
            segments.extend(["v1", "data"]);
            segments.extend(path.segments().iter().map(String::as_str));
        }
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }

    #[tracing::instrument(skip_all, fields(path))]
    async fn evaluate_inner(&self, path: &str, input: JsonValue) -> Result<JsonValue, PolicyError> {
        let path = PolicyPath::parse(path)?;
        let url = self.data_url(&path)?;
        let validated = validate_url(&url, &self.url_policy)
            .await
            .map_err(|error| PolicyError::InvalidUrl {
                message: error.to_string(),
            })?;
        let body = serde_json::to_vec(&json!({ "input": input })).map_err(|error| {
            PolicyError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let mut headers = self.headers.clone();
        if !headers.contains("content-type") {
            let _ = headers.insert("content-type", "application/json");
        }
        if !headers.contains("accept") {
            let _ = headers.insert("accept", "application/json");
        }
        let mut request = HttpRequest::new(Method::POST, url.clone())
            .with_headers(headers.with_user_agent_suffix([USER_AGENT]))
            .with_body(RequestBody::json(Bytes::from(body)))
            .with_pinned_addresses(validated.addresses);
        if let Some(timeout) = self.timeout {
            request = request.with_timeout(timeout);
        }
        let response = self
            .transport
            .execute(request)
            .await
            .map_err(|error| transport_error(&url, &error))?;
        let status = response.status;
        let bytes = read_body(&response.headers, response.body, self.max_response_bytes)
            .await
            .map_err(|error| transport_error(&url, &error))?;
        if !status.is_success() {
            return Err(PolicyError::Status {
                status,
                body: excerpt(&bytes),
            });
        }
        let document: JsonValue =
            serde_json::from_slice(&bytes).map_err(|error| PolicyError::InvalidResponse {
                message: error.to_string(),
            })?;
        match document {
            JsonValue::Object(mut object) => Ok(object.remove("result").unwrap_or(JsonValue::Null)),
            _ => Err(PolicyError::InvalidResponse {
                message: "expected a JSON object with a `result` member".to_owned(),
            }),
        }
    }
}

impl PolicyClient for HttpPolicyClient {
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>> {
        Box::pin(self.evaluate_inner(path, input))
    }
}

fn transport_error(url: &Url, error: &TransportError) -> PolicyError {
    PolicyError::Transport {
        host: url.host_str().unwrap_or_default().to_owned(),
        message: error.to_string(),
    }
}

fn excerpt(bytes: &[u8]) -> String {
    let end = bytes.len().min(BODY_EXCERPT_BYTES);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Builder of an [`HttpPolicyClient`].
pub struct HttpPolicyClientBuilder {
    base_url: Url,
    headers: Headers,
    transport: Option<SharedTransport>,
    url_policy: UrlPolicy,
    timeout: Option<Duration>,
    max_response_bytes: u64,
}

impl fmt::Debug for HttpPolicyClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpPolicyClientBuilder")
            .field("base_url", &self.base_url.origin().ascii_serialization())
            .field("headers", &self.headers.masked())
            .field("custom_transport", &self.transport.is_some())
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl HttpPolicyClientBuilder {
    /// Sets the headers sent with every request (for example an
    /// `authorization` header). Never logged unmasked.
    #[must_use]
    pub fn headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds one header; invalid names or values are skipped.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers = self.headers.with(name, value);
        self
    }

    /// Uses `transport` instead of the shared default transport.
    #[must_use]
    pub fn transport(mut self, transport: SharedTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Sets the URL policy applied to the server URL (default: HTTPS and
    /// public networks only).
    #[must_use]
    pub fn url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }

    /// Sets the per-request timeout (default: the transport's own timeout).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sets the response body limit (default 1 MiB).
    #[must_use]
    pub fn max_response_bytes(mut self, max_response_bytes: u64) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Transport`] when no transport was given and
    /// the default transport cannot be created.
    pub fn build(self) -> Result<HttpPolicyClient, PolicyError> {
        let transport = match self.transport {
            Some(transport) => transport,
            None => default_transport().map_err(|error| transport_error(&self.base_url, &error))?,
        };
        Ok(HttpPolicyClient {
            base_url: self.base_url,
            headers: self.headers,
            transport,
            url_policy: self.url_policy,
            timeout: self.timeout,
            max_response_bytes: self.max_response_bytes,
        })
    }
}
