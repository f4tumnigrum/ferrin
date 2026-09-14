//! Error type of the MCP client.

use std::time::Duration;

use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::secure_url::UrlValidationError;
use ferrin_spec::JsonValue;
use url::Url;

/// Boxed error cause.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Errors of the MCP client, transports and OAuth flow.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum McpError {
    /// A transport-level failure (connection, HTTP status, framing).
    #[error(transparent)]
    Transport(Box<TransportFailure>),
    /// The peer violated the protocol or returned an unparsable result.
    #[error("{message}")]
    Protocol {
        /// Explanation.
        message: String,
    },
    /// A JSON-RPC error response.
    #[error("server error {code}: {message}")]
    JsonRpc {
        /// JSON-RPC error code (`-32601`, `-32602`, MCP codes such as `-32002`).
        code: i64,
        /// Error message.
        message: String,
        /// Error data.
        data: Option<JsonValue>,
    },
    /// The request did not complete in time.
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
    /// The request was cancelled.
    #[error("request cancelled")]
    Cancelled,
    /// The client or transport is closed.
    #[error("client is closed")]
    Closed,
    /// The server requires authorization that has not been completed.
    #[error("unauthorized")]
    Unauthorized,
    /// The server did not declare the capability a method needs.
    #[error("server does not support {0}")]
    UnsupportedCapability(String),
    /// Invalid configuration or arguments.
    #[error("{message}")]
    InvalidArgument {
        /// Explanation.
        message: String,
    },
    /// A URL was rejected by the secure URL policy.
    #[error("url rejected: {0}")]
    Url(Box<UrlValidationError>),
    /// An OAuth failure.
    #[error("{message}")]
    OAuth {
        /// Explanation.
        message: String,
        /// OAuth error code (`invalid_grant`, ...), when the server sent one.
        error_code: Option<String>,
    },
    /// An I/O failure (stdio transport).
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Handling a server-initiated request failed.
    #[error("{message}")]
    Elicitation {
        /// Explanation.
        message: String,
    },
}

/// Details of [`McpError::Transport`].
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct TransportFailure {
    /// Explanation.
    pub message: String,
    /// HTTP status of the failing response, when the transport is HTTP.
    pub status_code: Option<u16>,
    /// Endpoint the failing request was sent to.
    pub url: Option<Url>,
    /// Body of the failing HTTP response, when available.
    pub response_body: Option<String>,
    /// Underlying error.
    #[source]
    pub cause: Option<BoxError>,
}

impl From<UrlValidationError> for McpError {
    fn from(error: UrlValidationError) -> Self {
        Self::Url(Box::new(error))
    }
}

impl McpError {
    /// A protocol error.
    #[must_use]
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
        }
    }

    /// A transport error without HTTP context.
    #[must_use]
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport(Box::new(TransportFailure {
            message: message.into(),
            status_code: None,
            url: None,
            response_body: None,
            cause: None,
        }))
    }

    /// A transport error carrying an HTTP status.
    #[must_use]
    pub fn http_status(
        message: impl Into<String>,
        status: http::StatusCode,
        url: &Url,
        response_body: Option<String>,
    ) -> Self {
        Self::Transport(Box::new(TransportFailure {
            message: message.into(),
            status_code: Some(status.as_u16()),
            url: Some(url.clone()),
            response_body,
            cause: None,
        }))
    }

    /// Wraps a [`TransportError`].
    #[must_use]
    pub fn from_transport(error: TransportError, url: &Url) -> Self {
        if error.is_cancelled() {
            return Self::Cancelled;
        }
        Self::Transport(Box::new(TransportFailure {
            message: format!(
                "request to {} failed: {}",
                url.host_str().unwrap_or_default(),
                error.message
            ),
            status_code: None,
            url: Some(url.clone()),
            response_body: None,
            cause: Some(Box::new(error)),
        }))
    }

    /// An invalid-argument error.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            message: message.into(),
        }
    }

    /// An OAuth error without a server error code.
    #[must_use]
    pub fn oauth(message: impl Into<String>) -> Self {
        Self::OAuth {
            message: message.into(),
            error_code: None,
        }
    }

    /// An elicitation error.
    #[must_use]
    pub fn elicitation(message: impl Into<String>) -> Self {
        Self::Elicitation {
            message: message.into(),
        }
    }

    /// JSON-RPC error code, for [`McpError::JsonRpc`].
    #[must_use]
    pub fn code(&self) -> Option<i64> {
        match self {
            Self::JsonRpc { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// HTTP status code, for HTTP transport failures.
    #[must_use]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Transport(failure) => failure.status_code,
            _ => None,
        }
    }

    /// Transport failure details, for [`McpError::Transport`].
    #[must_use]
    pub fn transport_failure(&self) -> Option<&TransportFailure> {
        match self {
            Self::Transport(failure) => Some(failure),
            _ => None,
        }
    }

    /// Whether a failed `tools/call` may be retried: HTTP 408, 409, 429 and
    /// 5xx, or a connection-level failure; JSON-RPC errors never are.
    #[must_use]
    pub fn is_retryable_tool_call(&self) -> bool {
        if let Some(status) = self.status_code() {
            return matches!(status, 408 | 409 | 429) || status >= 500;
        }
        match self {
            Self::JsonRpc { .. } => false,
            Self::Transport(failure) => failure
                .cause
                .as_ref()
                .and_then(|cause| cause.downcast_ref::<TransportError>())
                .is_some_and(TransportError::is_retryable),
            Self::Io(_) => true,
            _ => false,
        }
    }
}
