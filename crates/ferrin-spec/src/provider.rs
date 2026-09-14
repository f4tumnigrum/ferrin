//! Provider trait: the entry point that hands out models and services.

use std::sync::Arc;

use crate::dynamic::BatchRef;
use crate::dynamic::EmbeddingModelRef;
use crate::dynamic::FilesRef;
use crate::dynamic::ImageModelRef;
use crate::dynamic::LanguageModelRef;
use crate::dynamic::RealtimeFactoryRef;
use crate::dynamic::RerankingModelRef;
use crate::dynamic::SkillsRef;
use crate::dynamic::SpeechModelRef;
use crate::dynamic::SpeechTranslationModelRef;
use crate::dynamic::TranscriptionModelRef;
use crate::dynamic::VideoModelRef;
use crate::error::ModelKind;
use crate::error::NoSuchModelError;
use crate::shared::ProviderId;

/// A provider: creates model instances by id and exposes provider services.
///
/// Implement `language_model`, `embedding_model` and `image_model`; return
/// [`NoSuchModelError::unsupported_kind`] for kinds the provider does not
/// offer. The remaining lookups default to that error, and the service
/// accessors default to `None`.
pub trait Provider: Send + Sync + 'static {
    /// Provider identifier, for example `openai`.
    fn provider_id(&self) -> &ProviderId;

    /// Returns the language model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown.
    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError>;

    /// Returns the embedding model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown.
    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError>;

    /// Returns the image model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown.
    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError>;

    /// Returns the transcription model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown or the provider
    /// has no transcription models.
    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Transcription,
        ))
    }

    /// Returns the speech model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown or the provider
    /// has no speech models.
    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Speech,
        ))
    }

    /// Returns the reranking model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown or the provider
    /// has no reranking models.
    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Reranking,
        ))
    }

    /// Returns the video model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown or the provider
    /// has no video models.
    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Video,
        ))
    }

    /// Returns the speech translation model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown or the provider
    /// has no speech translation models.
    fn speech_translation_model(
        &self,
        model_id: &str,
    ) -> Result<SpeechTranslationModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::SpeechTranslation,
        ))
    }

    /// Returns the realtime factory, if the provider supports realtime sessions.
    fn realtime(&self) -> Option<RealtimeFactoryRef> {
        None
    }

    /// Returns the files service, if the provider supports file storage.
    fn files(&self) -> Option<FilesRef> {
        None
    }

    /// Returns the skills service, if the provider supports skills.
    fn skills(&self) -> Option<SkillsRef> {
        None
    }

    /// Returns the batch service, if the provider supports batch jobs.
    fn batch(&self) -> Option<BatchRef> {
        None
    }
}

/// Shared reference to a provider.
pub type ProviderRef = Arc<dyn Provider>;
