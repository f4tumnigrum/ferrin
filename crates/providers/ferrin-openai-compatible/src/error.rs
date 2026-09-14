//! Error bodies and stream error frames of OpenAI-compatible endpoints.
//!
//! Endpoints differ in how they report errors; [`ErrorStructure`] lets a
//! provider crate built on this one describe its own error body.

use std::fmt;
use std::sync::Arc;

use ferrin_provider_util::http::JsonErrorResponseHandler;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::language_model::StreamErrorCode;
use http::StatusCode;
use serde::Deserialize;
use url::Url;

/// Body of an OpenAI-shaped error response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiCompatibleErrorData {
    /// The error object.
    pub error: OpenAiCompatibleErrorDetail,
}

/// The `error` object; everything but `message` is optional because
/// compatible endpoints vary.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiCompatibleErrorDetail {
    /// Human-readable message.
    pub message: String,
    /// Error type.
    #[serde(rename = "type", default)]
    pub error_type: Option<String>,
    /// Offending parameter.
    #[serde(default)]
    pub param: Option<JsonValue>,
    /// Error code (string or number).
    #[serde(default)]
    pub code: Option<JsonValue>,
}

/// Describes the error body of an endpoint.
pub trait ErrorStructure: Send + Sync + fmt::Debug {
    /// Extracts the message of an error body; `None` when the body does
    /// not match the structure (the HTTP status reason is used instead).
    fn message(&self, body: &JsonValue) -> Option<String>;

    /// Overrides retryability of a failed response; `None` keeps the
    /// status-based default.
    fn is_retryable(&self, status: StatusCode, body: Option<&JsonValue>) -> Option<bool> {
        let _ = (status, body);
        None
    }
}

/// Shared handle to an [`ErrorStructure`].
pub type SharedErrorStructure = Arc<dyn ErrorStructure>;

/// The OpenAI error body: `{"error": {"message", "type"?, "param"?,
/// "code"?}}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultErrorStructure;

impl ErrorStructure for DefaultErrorStructure {
    fn message(&self, body: &JsonValue) -> Option<String> {
        serde_json::from_value::<OpenAiCompatibleErrorData>(body.clone())
            .ok()
            .map(|data| data.error.message)
    }
}

/// Failure handler decoding error bodies with `structure`.
#[must_use]
pub fn failed_response_handler(
    structure: SharedErrorStructure,
) -> JsonErrorResponseHandler<JsonValue> {
    let message_structure = Arc::clone(&structure);
    let handler = json_error_response_handler::<JsonValue>(move |body| {
        message_structure
            .message(body)
            .unwrap_or_else(|| "request failed".to_owned())
    });
    handler.with_is_retryable(move |head, body| {
        structure
            .is_retryable(head.status, body)
            .unwrap_or_else(|| ferrin_provider_util::retry::is_retryable_status(head.status))
    })
}

/// Error information extracted from a stream frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    /// Message.
    pub message: String,
    /// Code as text.
    pub code: Option<String>,
    /// Type.
    pub error_type: Option<String>,
}

/// Renders an error code as text.
#[must_use]
pub fn code_text(code: Option<&JsonValue>) -> Option<String> {
    match code? {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Extracts the error of a stream frame with `structure`; the code and type
/// are read from `frame.error` when present.
#[must_use]
pub fn parse_frame_error(structure: &dyn ErrorStructure, frame: &JsonValue) -> Option<FrameError> {
    let message = structure.message(frame)?;
    let detail = frame.get("error").and_then(JsonValue::as_object);
    Some(FrameError {
        message,
        code: detail.and_then(|d| code_text(d.get("code"))),
        error_type: detail
            .and_then(|d| d.get("type"))
            .and_then(JsonValue::as_str)
            .map(str::to_owned),
    })
}

impl FrameError {
    /// Infers the HTTP status from the code and type.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        if let Some(code) = self
            .code
            .as_deref()
            .filter(|code| code.len() == 3)
            .and_then(|code| code.parse::<u16>().ok())
            .filter(|code| (400..=599).contains(code))
        {
            return code;
        }
        let discriminator = format!(
            "{} {}",
            self.code.as_deref().unwrap_or_default(),
            self.error_type.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        let has = |term: &str| discriminator.contains(term);
        if has("insufficient_quota") || has("rate_limit") {
            429
        } else if has("authentication") {
            401
        } else if has("permission") {
            403
        } else if has("not_found") {
            404
        } else if has("invalid") || has("bad_request") || has("context_length") {
            400
        } else if has("overload") {
            503
        } else if has("timeout") {
            504
        } else {
            500
        }
    }

    /// Whether retrying may succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        if self.code.as_deref() == Some("insufficient_quota")
            || self.error_type.as_deref() == Some("insufficient_quota")
        {
            return false;
        }
        let status = self.status_code();
        status == 408 || status == 409 || status == 429 || status >= 500
    }

    /// Converts to a stream error part payload.
    #[must_use]
    pub fn to_stream_error(&self, frame: &JsonValue) -> StreamError {
        let mut error = StreamError::new(self.message.clone());
        error.error_type = self.error_type.clone();
        error.code = self.code.clone().map(StreamErrorCode::Text);
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

/// Stream error for a frame; falls back to a generic message when the frame
/// does not match `structure`.
#[must_use]
pub fn stream_error_for_frame(structure: &dyn ErrorStructure, frame: &JsonValue) -> StreamError {
    match parse_frame_error(structure, frame) {
        Some(error) => error.to_stream_error(frame),
        None => {
            let mut error = StreamError::new("stream error");
            error.data = Some(frame.clone());
            error
        }
    }
}

/// API call error for an error frame received before any output.
#[must_use]
pub fn early_error(structure: &dyn ErrorStructure, url: Url, frame: &JsonValue) -> ApiCallError {
    match parse_frame_error(structure, frame) {
        Some(error) => error.to_api_call_error(url, frame),
        None => ApiCallError::new("stream failed before any output was generated", url)
            .with_status(StatusCode::INTERNAL_SERVER_ERROR)
            .with_data(frame.clone())
            .retryable(false),
    }
}
