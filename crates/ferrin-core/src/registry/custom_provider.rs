//! Providers assembled from explicit model instances.

use std::collections::HashMap;
use std::fmt;

use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::FilesRef;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ModelKind;
use ferrin_spec::NoSuchModelError;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderRef;
use ferrin_spec::RerankingModelRef;
use ferrin_spec::SkillsRef;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::VideoModelRef;

/// Starts building a provider named `id`.
#[must_use]
pub fn custom_provider(id: impl Into<ProviderId>) -> CustomProviderBuilder {
    CustomProviderBuilder {
        provider: CustomProvider {
            id: id.into(),
            language_models: HashMap::new(),
            embedding_models: HashMap::new(),
            image_models: HashMap::new(),
            transcription_models: HashMap::new(),
            speech_models: HashMap::new(),
            reranking_models: HashMap::new(),
            video_models: HashMap::new(),
            speech_translation_models: HashMap::new(),
            files: None,
            skills: None,
            fallback: None,
        },
    }
}

/// A provider that serves pre-built model instances, optionally falling back
/// to another provider for unknown ids.
#[derive(Clone)]
pub struct CustomProvider {
    id: ProviderId,
    language_models: HashMap<String, LanguageModelRef>,
    embedding_models: HashMap<String, EmbeddingModelRef>,
    image_models: HashMap<String, ImageModelRef>,
    transcription_models: HashMap<String, TranscriptionModelRef>,
    speech_models: HashMap<String, SpeechModelRef>,
    reranking_models: HashMap<String, RerankingModelRef>,
    video_models: HashMap<String, VideoModelRef>,
    speech_translation_models: HashMap<String, SpeechTranslationModelRef>,
    files: Option<FilesRef>,
    skills: Option<SkillsRef>,
    fallback: Option<ProviderRef>,
}

impl fmt::Debug for CustomProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomProvider")
            .field("id", &self.id)
            .field(
                "language_models",
                &self.language_models.keys().collect::<Vec<_>>(),
            )
            .field(
                "embedding_models",
                &self.embedding_models.keys().collect::<Vec<_>>(),
            )
            .field(
                "image_models",
                &self.image_models.keys().collect::<Vec<_>>(),
            )
            .field("fallback", &self.fallback.is_some())
            .finish_non_exhaustive()
    }
}

macro_rules! resolve {
    ($self:ident, $map:ident, $fallback:ident, $model_id:expr, $kind:expr) => {{
        if let Some(model) = $self.$map.get($model_id) {
            return Ok(model.clone());
        }
        match &$self.fallback {
            Some(fallback) => fallback.$fallback($model_id),
            None => Err(NoSuchModelError::new($model_id, $kind).with_provider(&$self.id)),
        }
    }};
}

impl Provider for CustomProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        resolve!(
            self,
            language_models,
            language_model,
            model_id,
            ModelKind::Language
        )
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        resolve!(
            self,
            embedding_models,
            embedding_model,
            model_id,
            ModelKind::Embedding
        )
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        resolve!(self, image_models, image_model, model_id, ModelKind::Image)
    }

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        resolve!(
            self,
            transcription_models,
            transcription_model,
            model_id,
            ModelKind::Transcription
        )
    }

    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        resolve!(
            self,
            speech_models,
            speech_model,
            model_id,
            ModelKind::Speech
        )
    }

    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> {
        resolve!(
            self,
            reranking_models,
            reranking_model,
            model_id,
            ModelKind::Reranking
        )
    }

    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> {
        resolve!(self, video_models, video_model, model_id, ModelKind::Video)
    }

    fn speech_translation_model(
        &self,
        model_id: &str,
    ) -> Result<SpeechTranslationModelRef, NoSuchModelError> {
        resolve!(
            self,
            speech_translation_models,
            speech_translation_model,
            model_id,
            ModelKind::SpeechTranslation
        )
    }

    fn realtime(&self) -> Option<ferrin_spec::RealtimeFactoryRef> {
        self.fallback
            .as_ref()
            .and_then(|fallback| fallback.realtime())
    }

    fn files(&self) -> Option<FilesRef> {
        self.files
            .clone()
            .or_else(|| self.fallback.as_ref().and_then(|fallback| fallback.files()))
    }

    fn skills(&self) -> Option<SkillsRef> {
        self.skills.clone().or_else(|| {
            self.fallback
                .as_ref()
                .and_then(|fallback| fallback.skills())
        })
    }

    fn batch(&self) -> Option<ferrin_spec::BatchRef> {
        self.fallback.as_ref().and_then(|fallback| fallback.batch())
    }
}

/// Builder of a [`CustomProvider`].
#[derive(Debug)]
pub struct CustomProviderBuilder {
    provider: CustomProvider,
}

impl CustomProviderBuilder {
    /// Registers a language model under `id`.
    #[must_use]
    pub fn language_model(
        mut self,
        id: impl Into<String>,
        model: impl Into<LanguageModelRef>,
    ) -> Self {
        self.provider
            .language_models
            .insert(id.into(), model.into());
        self
    }

    /// Registers an embedding model under `id`.
    #[must_use]
    pub fn embedding_model(
        mut self,
        id: impl Into<String>,
        model: impl Into<EmbeddingModelRef>,
    ) -> Self {
        self.provider
            .embedding_models
            .insert(id.into(), model.into());
        self
    }

    /// Registers an image model under `id`.
    #[must_use]
    pub fn image_model(mut self, id: impl Into<String>, model: impl Into<ImageModelRef>) -> Self {
        self.provider.image_models.insert(id.into(), model.into());
        self
    }

    /// Registers a transcription model under `id`.
    #[must_use]
    pub fn transcription_model(
        mut self,
        id: impl Into<String>,
        model: impl Into<TranscriptionModelRef>,
    ) -> Self {
        self.provider
            .transcription_models
            .insert(id.into(), model.into());
        self
    }

    /// Registers a speech model under `id`.
    #[must_use]
    pub fn speech_model(mut self, id: impl Into<String>, model: impl Into<SpeechModelRef>) -> Self {
        self.provider.speech_models.insert(id.into(), model.into());
        self
    }

    /// Registers a reranking model under `id`.
    #[must_use]
    pub fn reranking_model(
        mut self,
        id: impl Into<String>,
        model: impl Into<RerankingModelRef>,
    ) -> Self {
        self.provider
            .reranking_models
            .insert(id.into(), model.into());
        self
    }

    /// Registers a video model under `id`.
    #[must_use]
    pub fn video_model(mut self, id: impl Into<String>, model: impl Into<VideoModelRef>) -> Self {
        self.provider.video_models.insert(id.into(), model.into());
        self
    }

    /// Registers a speech translation model under `id`.
    #[must_use]
    pub fn speech_translation_model(
        mut self,
        id: impl Into<String>,
        model: impl Into<SpeechTranslationModelRef>,
    ) -> Self {
        self.provider
            .speech_translation_models
            .insert(id.into(), model.into());
        self
    }

    /// Delegates unknown ids (and services) to `provider`.
    #[must_use]
    pub fn fallback(mut self, provider: ProviderRef) -> Self {
        self.provider.fallback = Some(provider);
        self
    }

    /// Sets the file service, taking precedence over the fallback provider.
    #[must_use]
    pub fn files(mut self, files: FilesRef) -> Self {
        self.provider.files = Some(files);
        self
    }

    /// Sets the skill service, taking precedence over the fallback provider.
    #[must_use]
    pub fn skills(mut self, skills: SkillsRef) -> Self {
        self.provider.skills = Some(skills);
        self
    }

    /// Builds the provider.
    #[must_use]
    pub fn build(self) -> CustomProvider {
        self.provider
    }
}
