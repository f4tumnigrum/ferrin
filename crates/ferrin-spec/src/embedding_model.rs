//! Embedding model interface.

use std::future::Future;

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::language_model::ResponseMetadata;
use crate::shared::Headers;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;

/// An embedding vector.
pub type Embedding = Vec<f64>;

/// A model that turns text into embedding vectors.
///
/// Implementations report their batching limits through
/// [`max_embeddings_per_call`](Self::max_embeddings_per_call) and
/// [`supports_parallel_calls`](Self::supports_parallel_calls); the core splits
/// inputs and schedules calls accordingly.
pub trait EmbeddingModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Maximum number of values per call, or `None` when unlimited.
    fn max_embeddings_per_call(&self) -> Option<usize>;

    /// Maximum total UTF-8 size of the values of one call in bytes, or
    /// `None` when unlimited.
    fn max_input_bytes_per_call(&self) -> Option<usize> {
        None
    }

    /// Whether several calls may run concurrently against this model.
    fn supports_parallel_calls(&self) -> bool;

    /// Embeds `options.values` in one provider call.
    fn do_embed(
        &self,
        options: EmbedOptions,
    ) -> impl Future<Output = Result<EmbedResult, ProviderError>> + Send;
}

/// Options for a single embedding call.
#[derive(Debug, Clone, Default)]
pub struct EmbedOptions {
    /// Texts to embed.
    pub values: Vec<String>,
    /// Additional request headers.
    pub headers: Headers,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl EmbedOptions {
    /// Creates options for `values`.
    #[must_use]
    pub fn new(values: Vec<String>) -> Self {
        Self {
            values,
            ..Self::default()
        }
    }
}

/// Token usage of an embedding call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    /// Input tokens consumed.
    pub tokens: u64,
}

/// Result of an embedding call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbedResult {
    /// One embedding per input value, in input order.
    pub embeddings: Vec<Embedding>,
    /// Token usage, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<EmbeddingUsage>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Response metadata (headers and body).
    #[serde(default)]
    pub response: ResponseMetadata,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
}
