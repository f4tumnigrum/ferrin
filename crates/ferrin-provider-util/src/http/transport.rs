//! The [`HttpTransport`] trait and its request/response types.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use http::Method;
use http::StatusCode;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Byte stream of a response body.
pub type BodyStream = BoxStream<'static, Result<Bytes, TransportError>>;

/// Shared transport handle.
pub type SharedTransport = Arc<dyn HttpTransport>;

/// Sends HTTP requests.
///
/// Implement this to route provider traffic through a proxy, a recording
/// layer or a test double. Implementations must not follow redirects (the
/// secure download logic validates every hop) and must honour
/// `HttpRequest::cancellation`, `HttpRequest::timeout` and
/// `HttpRequest::pinned_addresses` where the underlying client allows it.
pub trait HttpTransport: Send + Sync + 'static {
    /// Executes a request, returning the response head and a body stream.
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;
}

impl<T: HttpTransport + ?Sized> HttpTransport for Arc<T> {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        (**self).execute(request)
    }
}

/// An outgoing request.
#[derive(Debug)]
pub struct HttpRequest {
    /// HTTP method.
    pub method: Method,
    /// Target URL.
    pub url: Url,
    /// Request headers.
    pub headers: Headers,
    /// Request body.
    pub body: RequestBody,
    /// Cancels the request (and the body stream) when triggered.
    pub cancellation: CancellationToken,
    /// Total timeout for the request, if any.
    pub timeout: Option<Duration>,
    /// Addresses the host must resolve to (DNS pinning). Empty means
    /// resolve normally.
    pub pinned_addresses: Vec<SocketAddr>,
}

impl HttpRequest {
    /// Creates a request without headers or body.
    #[must_use]
    pub fn new(method: Method, url: Url) -> Self {
        Self {
            method,
            url,
            headers: Headers::new(),
            body: RequestBody::Empty,
            cancellation: CancellationToken::new(),
            timeout: None,
            pinned_addresses: Vec::new(),
        }
    }

    /// A `GET` request.
    #[must_use]
    pub fn get(url: Url) -> Self {
        Self::new(Method::GET, url)
    }

    /// A `POST` request.
    #[must_use]
    pub fn post(url: Url) -> Self {
        Self::new(Method::POST, url)
    }

    /// Sets the headers.
    #[must_use]
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Sets the body.
    #[must_use]
    pub fn with_body(mut self, body: RequestBody) -> Self {
        self.body = body;
        self
    }

    /// Sets the cancellation token.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Sets the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Pins the host to the given addresses.
    #[must_use]
    pub fn with_pinned_addresses(mut self, addresses: Vec<SocketAddr>) -> Self {
        self.pinned_addresses = addresses;
        self
    }
}

/// Body of an outgoing request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RequestBody {
    /// No body.
    Empty,
    /// Raw bytes with a content type.
    Bytes {
        /// `Content-Type` value.
        content_type: String,
        /// Payload.
        data: Bytes,
    },
    /// A multipart form.
    Multipart(MultipartForm),
}

impl RequestBody {
    /// A JSON body.
    #[must_use]
    pub fn json(data: Bytes) -> Self {
        Self::Bytes {
            content_type: "application/json".to_owned(),
            data,
        }
    }

    /// Returns the `Content-Type` this body requires, if any.
    #[must_use]
    pub fn content_type(&self) -> Option<String> {
        match self {
            Self::Empty => None,
            Self::Bytes { content_type, .. } => Some(content_type.clone()),
            Self::Multipart(form) => Some(form.content_type()),
        }
    }

    /// Encodes the body to bytes (empty for [`RequestBody::Empty`]).
    #[must_use]
    pub fn to_bytes(&self) -> Bytes {
        match self {
            Self::Empty => Bytes::new(),
            Self::Bytes { data, .. } => data.clone(),
            Self::Multipart(form) => form.encode(),
        }
    }
}

/// A `multipart/form-data` body.
#[derive(Debug, Clone)]
pub struct MultipartForm {
    parts: Vec<MultipartPart>,
    boundary: String,
}

/// One part of a [`MultipartForm`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MultipartPart {
    /// A text field.
    Field {
        /// Field name.
        name: String,
        /// Field value.
        value: String,
    },
    /// A file.
    File {
        /// Field name.
        name: String,
        /// File name sent in the disposition (`blob` when absent).
        filename: Option<String>,
        /// Media type (`application/octet-stream` when absent).
        media_type: Option<String>,
        /// File bytes.
        data: Bytes,
    },
}

impl Default for MultipartForm {
    fn default() -> Self {
        Self::new()
    }
}

impl MultipartForm {
    /// Creates an empty form with a random boundary.
    #[must_use]
    pub fn new() -> Self {
        Self {
            parts: Vec::new(),
            boundary: format!("ferrin-multipart-{}", crate::ids::generate_id()),
        }
    }

    /// Creates an empty form with a fixed boundary (for tests and snapshots).
    #[must_use]
    pub fn with_boundary(boundary: impl Into<String>) -> Self {
        Self {
            parts: Vec::new(),
            boundary: boundary.into(),
        }
    }

    /// Adds a text field.
    #[must_use]
    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.parts.push(MultipartPart::Field {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a file.
    #[must_use]
    pub fn file(
        mut self,
        name: impl Into<String>,
        filename: Option<String>,
        media_type: Option<String>,
        data: Bytes,
    ) -> Self {
        self.parts.push(MultipartPart::File {
            name: name.into(),
            filename,
            media_type,
            data,
        });
        self
    }

    /// The parts.
    #[must_use]
    pub fn parts(&self) -> &[MultipartPart] {
        &self.parts
    }

    /// The boundary.
    #[must_use]
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    /// The `Content-Type` header value.
    #[must_use]
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    /// A JSON summary of the fields (files shown as `<file:name>`), used in
    /// error reports.
    #[must_use]
    pub fn values(&self) -> JsonValue {
        let mut map = serde_json::Map::new();
        for part in &self.parts {
            match part {
                MultipartPart::Field { name, value } => {
                    map.insert(name.clone(), json!(value));
                }
                MultipartPart::File { name, filename, .. } => {
                    let label = filename.as_deref().unwrap_or(name);
                    map.insert(name.clone(), json!(format!("<file:{label}>")));
                }
            }
        }
        JsonValue::Object(map)
    }

    /// Encodes the form body.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut out = Vec::new();
        for part in &self.parts {
            out.extend_from_slice(b"--");
            out.extend_from_slice(self.boundary.as_bytes());
            out.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
            match part {
                MultipartPart::Field { name, value } => {
                    out.extend_from_slice(escape_header_value(name).as_bytes());
                    out.extend_from_slice(b"\"\r\n\r\n");
                    out.extend_from_slice(value.as_bytes());
                    out.extend_from_slice(b"\r\n");
                }
                MultipartPart::File {
                    name,
                    filename,
                    media_type,
                    data,
                } => {
                    out.extend_from_slice(escape_header_value(name).as_bytes());
                    out.extend_from_slice(b"\"; filename=\"");
                    out.extend_from_slice(
                        escape_header_value(filename.as_deref().unwrap_or("blob")).as_bytes(),
                    );
                    out.extend_from_slice(b"\"\r\nContent-Type: ");
                    let media_type = media_type.as_deref().unwrap_or("application/octet-stream");
                    out.extend(
                        media_type
                            .bytes()
                            .filter(|byte| *byte != b'\r' && *byte != b'\n'),
                    );
                    out.extend_from_slice(b"\r\n\r\n");
                    out.extend_from_slice(data);
                    out.extend_from_slice(b"\r\n");
                }
            }
        }
        out.extend_from_slice(b"--");
        out.extend_from_slice(self.boundary.as_bytes());
        out.extend_from_slice(b"--\r\n");
        Bytes::from(out)
    }
}

fn escape_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .flat_map(|ch| match ch {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            other => vec![other],
        })
        .collect()
}

/// Status and headers of a response.
#[derive(Debug, Clone)]
pub struct ResponseHead {
    /// Status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: Headers,
}

/// An incoming response.
pub struct HttpResponse {
    /// Status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: Headers,
    /// Body stream.
    pub body: BodyStream,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl HttpResponse {
    /// Creates a response from a complete body.
    #[must_use]
    pub fn from_bytes(status: StatusCode, headers: Headers, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body: Box::pin(futures_util::stream::once(std::future::ready(Ok(body)))),
        }
    }

    /// Creates a response from a body stream.
    #[must_use]
    pub fn from_stream(status: StatusCode, headers: Headers, body: BodyStream) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    /// Copies status and headers.
    #[must_use]
    pub fn head(&self) -> ResponseHead {
        ResponseHead {
            status: self.status,
            headers: self.headers.clone(),
        }
    }
}

/// Category of a [`TransportError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransportErrorKind {
    /// Could not connect (DNS, refused, unreachable).
    Connect,
    /// The request or body timed out.
    Timeout,
    /// The connection was reset or closed prematurely.
    Reset,
    /// Other I/O failure.
    Io,
    /// TLS handshake or certificate failure.
    Tls,
    /// The URL could not be used.
    InvalidUrl,
    /// The request could not be built (invalid header, body).
    InvalidRequest,
    /// The response body could not be read or decoded.
    Body,
    /// The response body exceeded the configured limit.
    BodyTooLarge,
    /// The request was cancelled.
    Cancelled,
    /// Anything else.
    Other,
}

/// Failure below the HTTP status layer.
#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct TransportError {
    /// Category.
    pub kind: TransportErrorKind,
    /// Explanation.
    pub message: String,
    /// Underlying error.
    #[source]
    pub cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl TransportError {
    /// Creates an error.
    #[must_use]
    pub fn new(kind: TransportErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: None,
        }
    }

    /// Attaches the underlying error.
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// A cancellation error.
    #[must_use]
    pub fn cancelled() -> Self {
        Self::new(TransportErrorKind::Cancelled, "request cancelled")
    }

    /// Connection-level failures (connect, timeout, reset, I/O) are
    /// retryable; TLS, URL and request construction failures are not.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            TransportErrorKind::Connect
                | TransportErrorKind::Timeout
                | TransportErrorKind::Reset
                | TransportErrorKind::Io
        )
    }

    /// Returns `true` for cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.kind == TransportErrorKind::Cancelled
    }
}
