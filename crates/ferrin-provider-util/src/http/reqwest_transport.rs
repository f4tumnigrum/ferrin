//! Default transport built on reqwest.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::future::Either;
use tokio_util::sync::WaitForCancellationFutureOwned;

use super::request_body::RequestBody;
use super::transport::HttpRequest;
use super::transport::HttpResponse;
use super::transport::HttpTransport;
use super::transport::SharedTransport;
use super::transport::TransportError;
use super::transport::TransportErrorKind;

/// [`HttpTransport`] implemented with `reqwest`.
///
/// Configuration: rustls TLS with the platform verifier, HTTP/2 via ALPN,
/// no automatic redirects, no default timeout. Requests with pinned
/// addresses use a dedicated client built with `resolve_to_addrs`.
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Builds the default client.
    ///
    /// # Errors
    ///
    /// Returns a [`TransportErrorKind::Tls`] error when the TLS backend
    /// cannot be initialised.
    pub fn new() -> Result<Self, TransportError> {
        Ok(Self {
            client: Self::builder().build().map_err(|error| {
                TransportError::new(TransportErrorKind::Tls, "failed to build HTTP client")
                    .with_cause(error)
            })?,
        })
    }

    /// Wraps an existing client. Automatic redirects should be disabled on it.
    #[must_use]
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// The client builder with Ferrin's defaults applied.
    pub fn builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
    }

    /// The underlying client.
    #[must_use]
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    fn pinned_client(
        host: &str,
        addresses: &[SocketAddr],
    ) -> Result<reqwest::Client, TransportError> {
        Self::builder()
            .resolve_to_addrs(host, addresses)
            .build()
            .map_err(|error| {
                TransportError::new(
                    TransportErrorKind::Tls,
                    "failed to build pinned HTTP client",
                )
                .with_cause(error)
            })
    }
}

/// Returns a process-wide shared default transport.
///
/// # Errors
///
/// Returns the client construction error (the same one on every call).
pub fn default_transport() -> Result<SharedTransport, TransportError> {
    static SHARED: OnceLock<Result<Arc<ReqwestTransport>, String>> = OnceLock::new();
    match SHARED.get_or_init(|| {
        ReqwestTransport::new()
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }) {
        Ok(transport) => Ok(Arc::clone(transport) as SharedTransport),
        Err(message) => Err(TransportError::new(
            TransportErrorKind::Tls,
            message.clone(),
        )),
    }
}

impl HttpTransport for ReqwestTransport {
    #[allow(
        clippy::disallowed_methods,
        reason = "this transport is the audited entry point for all reqwest calls"
    )]
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let HttpRequest {
                method,
                url,
                headers,
                body,
                cancellation,
                timeout,
                pinned_addresses,
            } = request;
            let client = if pinned_addresses.is_empty() {
                self.client.clone()
            } else {
                let host = url.host_str().ok_or_else(|| {
                    TransportError::new(TransportErrorKind::InvalidUrl, "url has no host")
                })?;
                Self::pinned_client(host, &pinned_addresses)?
            };
            let content_type = (!headers.contains("content-type"))
                .then(|| body.content_type())
                .flatten();
            let content_length = (!headers.contains("content-length"))
                .then(|| body.content_length())
                .flatten();
            let mut builder = client
                .request(method, url.clone())
                .headers(headers.into_map());
            if let Some(content_type) = content_type {
                builder = builder.header(http::header::CONTENT_TYPE, content_type);
            }
            if let Some(timeout) = timeout {
                builder = builder.timeout(timeout);
            }
            match body {
                RequestBody::Empty => {}
                RequestBody::Bytes { data, .. } => builder = builder.body(data),
                body @ (RequestBody::Multipart(_) | RequestBody::Stream { .. }) => {
                    if let Some(length) = content_length {
                        builder = builder.header(http::header::CONTENT_LENGTH, length);
                    }
                    let upload = CancellableBody {
                        inner: Some(body.into_stream()),
                        cancelled: Box::pin(cancellation.clone().cancelled_owned()),
                        finished: false,
                    };
                    builder = builder.body(reqwest::Body::wrap_stream(upload));
                }
            }
            let send = client.execute(builder.build().map_err(map_error)?);
            let response = match futures_util::future::select(
                Box::pin(cancellation.cancelled()),
                Box::pin(send),
            )
            .await
            {
                Either::Left(((), _)) => return Err(TransportError::cancelled()),
                Either::Right((response, _)) => response.map_err(map_error)?,
            };
            let status = response.status();
            let response_headers = Headers::from_map(response.headers().clone());
            let stream = response.bytes_stream().map(|item| item.map_err(map_error));
            let body = CancellableBody {
                inner: Some(Box::pin(stream)),
                cancelled: Box::pin(cancellation.cancelled_owned()),
                finished: false,
            };
            Ok(HttpResponse::from_stream(
                status,
                response_headers,
                Box::pin(body),
            ))
        })
    }
}

struct CancellableBody {
    inner: Option<super::transport::BodyStream>,
    cancelled: Pin<Box<WaitForCancellationFutureOwned>>,
    finished: bool,
}

impl Stream for CancellableBody {
    type Item = Result<Bytes, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        if self.cancelled.as_mut().poll(cx).is_ready() {
            self.finished = true;
            self.inner = None;
            return Poll::Ready(Some(Err(TransportError::cancelled())));
        }
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(None);
        };
        match inner.as_mut().poll_next(cx) {
            Poll::Ready(None) => {
                self.finished = true;
                self.inner = None;
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(error))) => {
                self.finished = true;
                self.inner = None;
                Poll::Ready(Some(Err(error)))
            }
            other => other,
        }
    }
}

fn map_error(error: reqwest::Error) -> TransportError {
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        if let Some(upload_error) = cause.downcast_ref::<TransportError>() {
            if upload_error.is_cancelled() {
                return TransportError::cancelled();
            }
            return TransportError::new(TransportErrorKind::Body, "request body stream failed")
                .with_cause(error);
        }
        source = cause.source();
    }
    let kind = if error.is_timeout() {
        TransportErrorKind::Timeout
    } else if error.is_connect() {
        TransportErrorKind::Connect
    } else if error.is_body() || error.is_decode() {
        TransportErrorKind::Body
    } else if error.is_builder() {
        TransportErrorKind::InvalidRequest
    } else if error.is_request() && mentions_reset(&error) {
        TransportErrorKind::Reset
    } else if error.is_request() {
        TransportErrorKind::Io
    } else {
        TransportErrorKind::Other
    };
    let message = error.to_string();
    TransportError::new(kind, message).with_cause(error)
}

fn mentions_reset(error: &reqwest::Error) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(err) = current {
        let text = err.to_string().to_ascii_lowercase();
        if text.contains("reset")
            || text.contains("broken pipe")
            || text.contains("connection closed")
        {
            return true;
        }
        current = err.source();
    }
    false
}
