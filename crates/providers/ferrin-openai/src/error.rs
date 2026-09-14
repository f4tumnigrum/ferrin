//! OpenAI error response schema and stream error frames.

use ferrin_provider_util::http::JsonErrorResponseHandler;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::language_model::StreamErrorCode;
use http::StatusCode;
use serde::Deserialize;
use url::Url;

/// Body of an OpenAI error response.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiErrorData {
    /// The error object.
    pub error: OpenAiErrorDetail,
}

/// The `error` object of an OpenAI error response.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiErrorDetail {
    /// Human-readable message.
    pub message: String,
    /// Error type (`invalid_request_error`, ...).
    #[serde(rename = "type", default)]
    pub error_type: Option<String>,
    /// Offending parameter, when reported.
    #[serde(default)]
    pub param: Option<JsonValue>,
    /// Error code (string or number).
    #[serde(default)]
    pub code: Option<JsonValue>,
}

/// Failure handler decoding [`OpenAiErrorData`] bodies.
#[must_use]
pub fn failed_response_handler() -> JsonErrorResponseHandler<OpenAiErrorData> {
    json_error_response_handler::<OpenAiErrorData>(|data| data.error.message.clone())
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

/// Extracts the error from a stream frame: `{type: "response.failed",
/// response: {error}}`, `{error: {...}}` or a bare `{message, code|type}`.
#[must_use]
pub fn parse_frame_error(frame: &JsonValue) -> Option<FrameError> {
    let object = frame.as_object()?;
    if object.get("type").and_then(JsonValue::as_str) == Some("response.failed") {
        let error = object.get("response")?.get("error")?.as_object()?;
        return Some(FrameError {
            message: error.get("message")?.as_str()?.to_owned(),
            code: code_text(error.get("code")),
            error_type: Some("response.failed".to_owned()),
        });
    }
    let nested = object.get("error").and_then(JsonValue::as_object);
    let error = nested.unwrap_or(object);
    let message = error.get("message")?.as_str()?;
    let has_marker = nested.is_some()
        || error.get("type").is_some_and(JsonValue::is_string)
        || error.contains_key("code")
        || error.contains_key("param");
    has_marker.then(|| FrameError {
        message: message.to_owned(),
        code: code_text(error.get("code")),
        error_type: error
            .get("type")
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

/// Stream error for an arbitrary frame; falls back to a generic message.
#[must_use]
pub fn stream_error_for_frame(frame: &JsonValue) -> StreamError {
    match parse_frame_error(frame) {
        Some(error) => error.to_stream_error(frame),
        None => {
            let mut error = StreamError::new("OpenAI stream error");
            error.data = Some(frame.clone());
            error
        }
    }
}
