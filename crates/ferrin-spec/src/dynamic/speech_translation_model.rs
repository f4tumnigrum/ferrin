//! Object-safe speech translation model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::speech_translation_model::SpeechTranslationModel;
use crate::speech_translation_model::SpeechTranslationStreamOptions;
use crate::speech_translation_model::SpeechTranslationStreamResult;

/// Object-safe counterpart of [`SpeechTranslationModel`].
pub trait DynSpeechTranslationModel: Send + Sync + 'static {
    /// See [`SpeechTranslationModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`SpeechTranslationModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`SpeechTranslationModel::do_stream`].
    fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> BoxFuture<'_, Result<SpeechTranslationStreamResult, ProviderError>>;
}

impl<T: SpeechTranslationModel> DynSpeechTranslationModel for T {
    fn provider(&self) -> &ProviderId {
        SpeechTranslationModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        SpeechTranslationModel::model_id(self)
    }

    fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> BoxFuture<'_, Result<SpeechTranslationStreamResult, ProviderError>> {
        Box::pin(SpeechTranslationModel::do_stream(self, options))
    }
}

/// Shared reference to a speech translation model (or an unresolved model id).
pub type SpeechTranslationModelRef = ModelRef<dyn DynSpeechTranslationModel>;

ref_conversions!(
    SpeechTranslationModelRef,
    SpeechTranslationModel,
    DynSpeechTranslationModel
);
