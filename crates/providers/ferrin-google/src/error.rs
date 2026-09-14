//! Google error response schema.

use ferrin_provider_util::http::JsonErrorResponseHandler;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_spec::JsonValue;
use serde::Deserialize;

/// Body of a Google error response: `{error: {code, message, status, details}}`.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleErrorData {
    /// The error object.
    pub error: GoogleErrorDetail,
}

/// The `error` object of a Google error response.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleErrorDetail {
    /// HTTP status code echoed by the API.
    #[serde(default)]
    pub code: Option<i64>,
    /// Human-readable message.
    #[serde(default)]
    pub message: Option<String>,
    /// gRPC status name (`RESOURCE_EXHAUSTED`, `INVALID_ARGUMENT`, ...).
    #[serde(default)]
    pub status: Option<String>,
    /// Structured details (`google.rpc.RetryInfo`, `google.rpc.QuotaFailure`, ...).
    #[serde(default)]
    pub details: Option<Vec<JsonValue>>,
}

/// Message shown for an error body.
#[must_use]
pub fn error_message(data: &GoogleErrorData) -> String {
    data.error
        .message
        .clone()
        .unwrap_or_else(|| "Google Generative AI request failed".to_owned())
}

/// Failure handler decoding [`GoogleErrorData`] bodies.
#[must_use]
pub fn failed_response_handler() -> JsonErrorResponseHandler<GoogleErrorData> {
    json_error_response_handler::<GoogleErrorData>(error_message)
}
