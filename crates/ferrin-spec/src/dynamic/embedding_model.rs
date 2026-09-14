//! Object-safe embedding model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::embedding_model::EmbedOptions;
use crate::embedding_model::EmbedResult;
use crate::embedding_model::EmbeddingModel;
use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`EmbeddingModel`].
pub trait DynEmbeddingModel: Send + Sync + 'static {
    /// See [`EmbeddingModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`EmbeddingModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`EmbeddingModel::max_embeddings_per_call`].
    fn max_embeddings_per_call(&self) -> Option<usize>;
    /// See [`EmbeddingModel::max_input_bytes_per_call`].
    fn max_input_bytes_per_call(&self) -> Option<usize>;
    /// See [`EmbeddingModel::supports_parallel_calls`].
    fn supports_parallel_calls(&self) -> bool;
    /// See [`EmbeddingModel::do_embed`].
    fn do_embed(&self, options: EmbedOptions) -> BoxFuture<'_, Result<EmbedResult, ProviderError>>;
}

impl<T: EmbeddingModel> DynEmbeddingModel for T {
    fn provider(&self) -> &ProviderId {
        EmbeddingModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        EmbeddingModel::model_id(self)
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        EmbeddingModel::max_embeddings_per_call(self)
    }

    fn max_input_bytes_per_call(&self) -> Option<usize> {
        EmbeddingModel::max_input_bytes_per_call(self)
    }

    fn supports_parallel_calls(&self) -> bool {
        EmbeddingModel::supports_parallel_calls(self)
    }

    fn do_embed(&self, options: EmbedOptions) -> BoxFuture<'_, Result<EmbedResult, ProviderError>> {
        Box::pin(EmbeddingModel::do_embed(self, options))
    }
}

/// Shared reference to an embedding model (or an unresolved model id).
pub type EmbeddingModelRef = ModelRef<dyn DynEmbeddingModel>;

ref_conversions!(EmbeddingModelRef, EmbeddingModel, DynEmbeddingModel);
