//! Object-safe reranking model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::reranking_model::RerankOptions;
use crate::reranking_model::RerankResult;
use crate::reranking_model::RerankingModel;
use crate::shared::ModelId;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`RerankingModel`].
pub trait DynRerankingModel: Send + Sync + 'static {
    /// See [`RerankingModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`RerankingModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`RerankingModel::do_rerank`].
    fn do_rerank(
        &self,
        options: RerankOptions,
    ) -> BoxFuture<'_, Result<RerankResult, ProviderError>>;
}

impl<T: RerankingModel> DynRerankingModel for T {
    fn provider(&self) -> &ProviderId {
        RerankingModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        RerankingModel::model_id(self)
    }

    fn do_rerank(
        &self,
        options: RerankOptions,
    ) -> BoxFuture<'_, Result<RerankResult, ProviderError>> {
        Box::pin(RerankingModel::do_rerank(self, options))
    }
}

/// Shared reference to a reranking model (or an unresolved model id).
pub type RerankingModelRef = ModelRef<dyn DynRerankingModel>;

ref_conversions!(RerankingModelRef, RerankingModel, DynRerankingModel);
