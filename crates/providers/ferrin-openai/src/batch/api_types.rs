//! Wire types of the Batch API.

use ferrin_spec::JsonValue;
use serde::Deserialize;

/// Batch-level provider options.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProviderOptions {
    /// Lifetime of the uploaded input file in seconds (3600..=2592000).
    #[serde(default)]
    pub input_file_expires_after: Option<u64>,
}

/// Request counts of a batch.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OpenAiBatchRequestCounts {
    /// Total.
    #[serde(default)]
    pub total: Option<u64>,
    /// Completed.
    #[serde(default)]
    pub completed: Option<u64>,
    /// Failed.
    #[serde(default)]
    pub failed: Option<u64>,
}

/// Batch-level error entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OpenAiBatchErrorEntry {
    /// Code.
    #[serde(default)]
    pub code: Option<String>,
    /// Message.
    #[serde(default)]
    pub message: Option<String>,
}

/// Batch-level errors.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OpenAiBatchErrors {
    /// Entries.
    #[serde(default)]
    pub data: Option<Vec<OpenAiBatchErrorEntry>>,
}

/// Batch object.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiBatchObject {
    /// Id.
    pub id: String,
    /// Raw status.
    pub status: String,
    /// Output file.
    #[serde(default)]
    pub output_file_id: Option<String>,
    /// Error file.
    #[serde(default)]
    pub error_file_id: Option<String>,
    /// Creation time (Unix seconds).
    #[serde(default)]
    pub created_at: Option<f64>,
    /// Expiry (Unix seconds).
    #[serde(default)]
    pub expires_at: Option<f64>,
    /// Request counts.
    #[serde(default)]
    pub request_counts: Option<OpenAiBatchRequestCounts>,
    /// Errors.
    #[serde(default)]
    pub errors: Option<OpenAiBatchErrors>,
}

/// Page of batches.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiBatchList {
    /// Batches.
    #[serde(default)]
    pub data: Vec<OpenAiBatchObject>,
    /// More pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// Cursor of the last item.
    #[serde(default)]
    pub last_id: Option<String>,
}

/// Response part of a result line.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchLineResponse {
    /// HTTP status of the request.
    pub status_code: u16,
    /// Request id.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Response body.
    #[serde(default)]
    pub body: Option<JsonValue>,
}

/// Error part of a result line.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchLineError {
    /// Code.
    #[serde(default)]
    pub code: Option<String>,
    /// Message.
    #[serde(default)]
    pub message: Option<String>,
}

/// One line of an output or error file.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResultLine {
    /// Request id given at start.
    pub custom_id: String,
    /// Response.
    #[serde(default)]
    pub response: Option<BatchLineResponse>,
    /// Error.
    #[serde(default)]
    pub error: Option<BatchLineError>,
}
