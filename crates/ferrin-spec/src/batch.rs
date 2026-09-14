//! Provider batch processing interface.

use std::future::Future;

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::dynamic::BoxStream;
use crate::error::ProviderError;
use crate::image_model::AspectRatio;
use crate::image_model::ImageFile;
use crate::image_model::ImageResult;
use crate::image_model::ImageSize;
use crate::language_model::GenerateResult;
use crate::language_model::Prompt;
use crate::language_model::ReasoningEffort;
use crate::language_model::ResponseFormat;
use crate::language_model::SupportedUrls;
use crate::language_model::ToolChoice;
use crate::language_model::ToolDefinition;
use crate::shared::BatchId;
use crate::shared::Headers;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;

/// Stream of per-request results of a finished batch.
pub type BatchResultStream = BoxStream<'static, Result<BatchItemResult, ProviderError>>;

/// Submit many text or image requests as one provider batch job.
pub trait Batch: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// URL patterns the provider can fetch itself (see `LanguageModel`).
    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send;

    /// Starts a batch job.
    fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> impl Future<Output = Result<BatchStartResult, ProviderError>> + Send;

    /// Returns the normalized status of a batch job.
    fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> impl Future<Output = Result<BatchStatus, ProviderError>> + Send;

    /// Streams the results of a finished batch job.
    fn do_get_batch_results(
        &self,
        options: BatchOperationOptions,
    ) -> impl Future<Output = Result<BatchResultStream, ProviderError>> + Send;

    /// Whether [`do_cancel_batch`](Self::do_cancel_batch) is implemented.
    fn supports_cancel_batch(&self) -> bool {
        false
    }

    /// Cancels a batch job.
    fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> impl Future<Output = Result<BatchCancelResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("cancel_batch")))
    }

    /// Whether [`do_list_batches`](Self::do_list_batches) is implemented.
    fn supports_list_batches(&self) -> bool {
        false
    }

    /// Lists batch jobs.
    fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> impl Future<Output = Result<BatchListResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("list_batches")))
    }
}

/// Settings of a text request inside a batch (no headers, no cancellation).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextBatchRequestOptions {
    /// The prompt.
    pub prompt: Prompt,
    /// Maximum output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Top-p.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Presence penalty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Frequency penalty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Reasoning effort.
    #[serde(default)]
    pub reasoning: ReasoningEffort,
    /// Response format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Tool choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "ProviderOptions::is_empty")]
    pub provider_options: ProviderOptions,
}

/// Settings of an image request inside a batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageBatchRequestOptions {
    /// Prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Number of images.
    pub n: u32,
    /// Size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<ImageSize>,
    /// Aspect ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
    /// Seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Input files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ImageFile>,
    /// Mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<ImageFile>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "ProviderOptions::is_empty")]
    pub provider_options: ProviderOptions,
}

/// One request of a batch, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum BatchRequest {
    /// A text generation request.
    Text {
        /// Caller-assigned request id, echoed in results.
        id: String,
        /// Model to use.
        model_id: ModelId,
        /// Settings.
        options: TextBatchRequestOptions,
    },
    /// An image generation request.
    Image {
        /// Caller-assigned request id, echoed in results.
        id: String,
        /// Model to use.
        model_id: ModelId,
        /// Settings.
        options: ImageBatchRequestOptions,
    },
}

impl BatchRequest {
    /// Returns the caller-assigned request id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } | Self::Image { id, .. } => id,
        }
    }
}

/// Options for starting a batch.
#[derive(Debug, Clone)]
pub struct BatchStartOptions {
    /// Requests to submit.
    pub requests: Vec<BatchRequest>,
    /// Webhook URL to notify on completion.
    pub webhook_url: Option<Url>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

/// Options for status, results and cancel calls.
#[derive(Debug, Clone)]
pub struct BatchOperationOptions {
    /// The batch job.
    pub batch_id: BatchId,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl BatchOperationOptions {
    /// Creates options for `batch_id`.
    #[must_use]
    pub fn new(batch_id: impl Into<BatchId>) -> Self {
        Self {
            batch_id: batch_id.into(),
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Options for listing batches.
#[derive(Debug, Clone, Default)]
pub struct BatchListOptions {
    /// Page size.
    pub limit: Option<usize>,
    /// Cursor from a previous page.
    pub cursor: Option<String>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

/// Normalized batch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum BatchState {
    /// Queued or running.
    Pending,
    /// Finished; results are available.
    Completed,
    /// Failed as a whole.
    Failed,
}

/// Error reported for a batch or a batch item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchError {
    /// Message.
    pub message: String,
    /// Provider error type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    /// Provider error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// HTTP status code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

/// Request counts of a batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRequestCounts {
    /// Total requests.
    pub total: u64,
    /// Requests not finished yet.
    pub pending: u64,
    /// Requests finished successfully.
    pub completed: u64,
    /// Requests that failed.
    pub failed: u64,
}

/// Normalized status of a batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchStatus {
    /// Normalized state.
    pub status: BatchState,
    /// Provider's raw status string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_status: Option<String>,
    /// Request counts, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_counts: Option<BatchRequestCounts>,
    /// Batch-level error, if failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BatchError>,
    /// Creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    /// Expiry time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl BatchStatus {
    /// Creates a status with only the normalized state set.
    #[must_use]
    pub fn new(status: BatchState) -> Self {
        Self {
            status,
            raw_status: None,
            request_counts: None,
            error: None,
            created_at: None,
            expires_at: None,
            provider_metadata: None,
        }
    }
}

/// A warning attached to a batch start, optionally scoped to one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchWarning {
    /// Request the warning applies to, or `None` for the whole batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The warning.
    pub warning: Warning,
}

/// Result of starting a batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchStartResult {
    /// Provider batch id.
    pub batch_id: BatchId,
    /// Initial status.
    #[serde(flatten)]
    pub status: BatchStatus,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<BatchWarning>,
}

/// Result of cancelling a batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchCancelResult {
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// One entry of a batch listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchListItem {
    /// Provider batch id.
    pub batch_id: BatchId,
    /// Status.
    #[serde(flatten)]
    pub status: BatchStatus,
}

/// Result of listing batches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchListResult {
    /// Batches on this page.
    pub batches: Vec<BatchListItem>,
    /// Cursor for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Outcome of one request, tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
#[non_exhaustive]
pub enum BatchItem<R> {
    /// The request succeeded.
    Succeeded {
        /// Caller-assigned request id.
        id: String,
        /// The result.
        result: R,
    },
    /// The request failed.
    Failed {
        /// Caller-assigned request id.
        id: String,
        /// The error.
        error: BatchError,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// The request was cancelled.
    Cancelled {
        /// Caller-assigned request id.
        id: String,
        /// Error details, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<BatchError>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// The request expired before completion.
    Expired {
        /// Caller-assigned request id.
        id: String,
        /// Error details, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<BatchError>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

impl<R> BatchItem<R> {
    /// Returns the caller-assigned request id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Succeeded { id, .. }
            | Self::Failed { id, .. }
            | Self::Cancelled { id, .. }
            | Self::Expired { id, .. } => id,
        }
    }
}

/// Outcome of one request, tagged by request `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum BatchItemResult {
    /// A text request.
    Text(Box<BatchItem<GenerateResult>>),
    /// An image request.
    Image(Box<BatchItem<ImageResult>>),
}

impl BatchItemResult {
    /// Returns the caller-assigned request id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Text(item) => item.id(),
            Self::Image(item) => item.id(),
        }
    }
}
