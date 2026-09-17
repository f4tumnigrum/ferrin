//! The [`HttpTransport`] trait and its request/response types.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use http::Method;
use http::StatusCode;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::request_body::RequestBody;

/// Byte stream of an incoming or outgoing HTTP body.
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
    ///
    /// Consume request bodies through [`RequestBody::into_stream`] to support
    /// uploads without collecting their file contents first.
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
