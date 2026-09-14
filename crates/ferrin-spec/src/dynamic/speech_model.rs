//! Object-safe speech model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::speech_model::SpeechModel;
use crate::speech_model::SpeechOptions;
use crate::speech_model::SpeechResult;

/// Object-safe counterpart of [`SpeechModel`].
pub trait DynSpeechModel: Send + Sync + 'static {
    /// See [`SpeechModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`SpeechModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`SpeechModel::do_generate`].
    fn do_generate(
        &self,
        options: SpeechOptions,
    ) -> BoxFuture<'_, Result<SpeechResult, ProviderError>>;
}

impl<T: SpeechModel> DynSpeechModel for T {
    fn provider(&self) -> &ProviderId {
        SpeechModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        SpeechModel::model_id(self)
    }

    fn do_generate(
        &self,
        options: SpeechOptions,
    ) -> BoxFuture<'_, Result<SpeechResult, ProviderError>> {
        Box::pin(SpeechModel::do_generate(self, options))
    }
}

/// Shared reference to a speech model (or an unresolved model id).
pub type SpeechModelRef = ModelRef<dyn DynSpeechModel>;

ref_conversions!(SpeechModelRef, SpeechModel, DynSpeechModel);
