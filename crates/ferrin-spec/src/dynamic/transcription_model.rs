//! Object-safe transcription model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::transcription_model::TranscriptionModel;
use crate::transcription_model::TranscriptionOptions;
use crate::transcription_model::TranscriptionResult;
use crate::transcription_model::TranscriptionStreamOptions;
use crate::transcription_model::TranscriptionStreamResult;

/// Object-safe counterpart of [`TranscriptionModel`].
pub trait DynTranscriptionModel: Send + Sync + 'static {
    /// See [`TranscriptionModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`TranscriptionModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`TranscriptionModel::do_generate`].
    fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> BoxFuture<'_, Result<TranscriptionResult, ProviderError>>;
    /// See [`TranscriptionModel::supports_stream`].
    fn supports_stream(&self) -> bool;
    /// See [`TranscriptionModel::do_stream`].
    fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> BoxFuture<'_, Result<TranscriptionStreamResult, ProviderError>>;
}

impl<T: TranscriptionModel> DynTranscriptionModel for T {
    fn provider(&self) -> &ProviderId {
        TranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        TranscriptionModel::model_id(self)
    }

    fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> BoxFuture<'_, Result<TranscriptionResult, ProviderError>> {
        Box::pin(TranscriptionModel::do_generate(self, options))
    }

    fn supports_stream(&self) -> bool {
        TranscriptionModel::supports_stream(self)
    }

    fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> BoxFuture<'_, Result<TranscriptionStreamResult, ProviderError>> {
        Box::pin(TranscriptionModel::do_stream(self, options))
    }
}

/// Shared reference to a transcription model (or an unresolved model id).
pub type TranscriptionModelRef = ModelRef<dyn DynTranscriptionModel>;

ref_conversions!(
    TranscriptionModelRef,
    TranscriptionModel,
    DynTranscriptionModel
);
