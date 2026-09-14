//! Object-safe language model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::language_model::CallOptions;
use crate::language_model::GenerateResult;
use crate::language_model::LanguageModel;
use crate::language_model::StreamResult;
use crate::language_model::SupportedUrls;
use crate::shared::ModelId;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`LanguageModel`].
///
/// Implemented automatically for every `LanguageModel`; do not implement it
/// by hand.
pub trait DynLanguageModel: Send + Sync + 'static {
    /// See [`LanguageModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`LanguageModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`LanguageModel::supported_urls`].
    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls>;
    /// See [`LanguageModel::do_generate`].
    fn do_generate(
        &self,
        options: CallOptions,
    ) -> BoxFuture<'_, Result<GenerateResult, ProviderError>>;
    /// See [`LanguageModel::do_stream`].
    fn do_stream(&self, options: CallOptions)
    -> BoxFuture<'_, Result<StreamResult, ProviderError>>;
}

impl<T: LanguageModel> DynLanguageModel for T {
    fn provider(&self) -> &ProviderId {
        LanguageModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        LanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls> {
        Box::pin(LanguageModel::supported_urls(self))
    }

    fn do_generate(
        &self,
        options: CallOptions,
    ) -> BoxFuture<'_, Result<GenerateResult, ProviderError>> {
        Box::pin(LanguageModel::do_generate(self, options))
    }

    fn do_stream(
        &self,
        options: CallOptions,
    ) -> BoxFuture<'_, Result<StreamResult, ProviderError>> {
        Box::pin(LanguageModel::do_stream(self, options))
    }
}

/// Shared reference to a language model (or an unresolved model id).
pub type LanguageModelRef = ModelRef<dyn DynLanguageModel>;

ref_conversions!(LanguageModelRef, LanguageModel, DynLanguageModel);
