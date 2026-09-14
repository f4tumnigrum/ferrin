//! HTTP call failures.

use http::StatusCode;
use url::Url;

use super::BoxError;
use super::truncate_for_display;
use crate::json::JsonValue;
use crate::shared::Headers;

/// Maximum number of bytes of the message shown by `Display`.
const DISPLAY_MAX_BYTES: usize = 2048;

/// Returns the default retry classification for an HTTP status.
///
/// Retryable: 408 (request timeout), 409 (conflict), 429 (too many requests)
/// and every 5xx status. `None` (no HTTP response, e.g. a network error) is
/// treated as retryable.
#[must_use]
pub fn default_retryable(status_code: Option<StatusCode>) -> bool {
    match status_code {
        None => true,
        Some(status) => {
            status == StatusCode::REQUEST_TIMEOUT
                || status == StatusCode::CONFLICT
                || status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
        }
    }
}

/// An HTTP request to the provider failed or returned an error status.
#[derive(Debug)]
pub struct ApiCallError {
    /// Human-readable message (no secrets, no full bodies).
    pub message: String,
    /// Request URL.
    pub url: Url,
    /// JSON request body that was sent, if any.
    pub request_body: Option<JsonValue>,
    /// HTTP status code, if a response was received.
    pub status_code: Option<StatusCode>,
    /// Response headers, if a response was received.
    pub response_headers: Option<Headers>,
    /// Raw response body text, if a response was received.
    pub response_body: Option<String>,
    /// Whether retrying may succeed.
    pub is_retryable: bool,
    /// Structured error payload parsed from the response, if any.
    pub data: Option<JsonValue>,
    /// Underlying cause (network error, JSON error, ...).
    pub cause: Option<BoxError>,
}

impl ApiCallError {
    /// Creates an error without a response; retryable by default.
    #[must_use]
    pub fn new(message: impl Into<String>, url: Url) -> Self {
        Self {
            message: message.into(),
            url,
            request_body: None,
            status_code: None,
            response_headers: None,
            response_body: None,
            is_retryable: default_retryable(None),
            data: None,
            cause: None,
        }
    }

    /// Sets the status code and recomputes the default retry classification.
    #[must_use]
    pub fn with_status(mut self, status_code: StatusCode) -> Self {
        self.status_code = Some(status_code);
        self.is_retryable = default_retryable(Some(status_code));
        self
    }

    /// Sets the request body.
    #[must_use]
    pub fn with_request_body(mut self, body: JsonValue) -> Self {
        self.request_body = Some(body);
        self
    }

    /// Sets response headers and body.
    #[must_use]
    pub fn with_response(mut self, headers: Headers, body: Option<String>) -> Self {
        self.response_headers = Some(headers);
        self.response_body = body;
        self
    }

    /// Sets the structured error payload.
    #[must_use]
    pub fn with_data(mut self, data: JsonValue) -> Self {
        self.data = Some(data);
        self
    }

    /// Sets the underlying cause.
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// Overrides the retry classification.
    #[must_use]
    pub fn retryable(mut self, is_retryable: bool) -> Self {
        self.is_retryable = is_retryable;
        self
    }
}

impl std::fmt::Display for ApiCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&truncate_for_display(&self.message, DISPLAY_MAX_BYTES))
    }
}

impl std::error::Error for ApiCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}
