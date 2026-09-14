//! Anthropic error response schema and stream error frames.

use ferrin_provider_util::http::JsonErrorResponseHandler;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::language_model::StreamError;
use http::StatusCode;
use serde::Deserialize;
use url::Url;

/// Body of an Anthropic error response: `{type: "error", error: {type, message}}`.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicErrorData {
    /// Always `error`.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// The error object.
    pub error: AnthropicErrorDetail,
}

/// The `error` object of an Anthropic error response.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicErrorDetail {
    /// Error type (`invalid_request_error`, `overloaded_error`, ...).
    #[serde(rename = "type", default)]
    pub error_type: Option<String>,
    /// Human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

/// Failure handler decoding [`AnthropicErrorData`] bodies.
#[must_use]
pub fn failed_response_handler() -> JsonErrorResponseHandler<AnthropicErrorData> {
    json_error_response_handler::<AnthropicErrorData>(|data| {
        data.error
            .message
            .clone()
            .unwrap_or_else(|| "Anthropic request failed".to_owned())
    })
}

/// HTTP status and retryability implied by a stream error type.
#[must_use]
pub fn status_for_error_type(error_type: Option<&str>) -> (u16, bool) {
    match error_type {
        Some("api_error") => (500, true),
        Some("overloaded_error") => (529, true),
        Some("rate_limit_error") => (429, true),
        Some("request_too_large") => (413, false),
        Some("authentication_error") => (401, false),
        Some("permission_error") => (403, false),
        Some("not_found_error") => (404, false),
        Some("billing_error" | "invalid_request_error") => (400, false),
        _ => (500, false),
    }
}

/// Error information extracted from a stream `error` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    /// Message.
    pub message: String,
    /// Type.
    pub error_type: Option<String>,
}

impl FrameError {
    /// Reads `{type, message}` from the `error` object of a frame.
    #[must_use]
    pub fn from_error_object(error: &JsonValue) -> Self {
        Self {
            message: error
                .get("message")
                .and_then(JsonValue::as_str)
                .unwrap_or("Anthropic stream error")
                .to_owned(),
            error_type: error
                .get("type")
                .and_then(JsonValue::as_str)
                .map(str::to_owned),
        }
    }

    /// Inferred HTTP status.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        status_for_error_type(self.error_type.as_deref()).0
    }

    /// Whether retrying may succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        status_for_error_type(self.error_type.as_deref()).1
    }

    /// Converts to a stream error part payload.
    #[must_use]
    pub fn to_stream_error(&self, frame: &JsonValue) -> StreamError {
        let mut error = StreamError::new(self.message.clone());
        error.error_type = self.error_type.clone();
        error.status_code = Some(self.status_code());
        error.is_retryable = Some(self.is_retryable());
        error.data = Some(frame.clone());
        error
    }

    /// Converts to an API call error for failures before any output.
    #[must_use]
    pub fn to_api_call_error(&self, url: Url, frame: &JsonValue) -> ApiCallError {
        ApiCallError::new(self.message.clone(), url)
            .with_status(
                StatusCode::from_u16(self.status_code())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            )
            .with_response(ferrin_spec::Headers::new(), Some(frame.to_string()))
            .with_data(frame.clone())
            .retryable(self.is_retryable())
    }
}
