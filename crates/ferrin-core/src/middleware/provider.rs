//! Application of middleware to every model a provider resolves.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::BatchRef;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::FilesRef;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ModelKind;
use ferrin_spec::ModelRef;
use ferrin_spec::NoSuchModelError;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderRef;
use ferrin_spec::RealtimeFactoryRef;
use ferrin_spec::RerankingModelRef;
use ferrin_spec::SkillsRef;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::VideoModelRef;

use super::EmbeddingModelMiddleware;
use super::ImageModelMiddleware;
use super::LanguageModelMiddleware;
use super::wrap_embedding_model;
use super::wrap_image_model;
use super::wrap_language_model;

/// Middleware applied by [`wrap_provider`] to the models a provider resolves.
///
/// Each list is applied in order (first outermost) to every model of its
/// kind; models of other kinds and provider services pass through.
#[derive(Clone, Default)]
pub struct ProviderMiddleware {
    /// Applied to every language model.
    pub language_model: Vec<Arc<dyn LanguageModelMiddleware>>,
    /// Applied to every embedding model.
    pub embedding_model: Vec<Arc<dyn EmbeddingModelMiddleware>>,
    /// Applied to every image model.
    pub image_model: Vec<Arc<dyn ImageModelMiddleware>>,
}

impl fmt::Debug for ProviderMiddleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderMiddleware")
            .field("language_model", &self.language_model.len())
            .field("embedding_model", &self.embedding_model.len())
            .field("image_model", &self.image_model.len())
            .finish()
    }
}

impl ProviderMiddleware {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a language model middleware.
    #[must_use]
    pub fn language_model(mut self, middleware: Arc<dyn LanguageModelMiddleware>) -> Self {
        self.language_model.push(middleware);
        self
    }

    /// Appends an embedding model middleware.
    #[must_use]
    pub fn embedding_model(mut self, middleware: Arc<dyn EmbeddingModelMiddleware>) -> Self {
        self.embedding_model.push(middleware);
        self
    }

    /// Appends an image model middleware.
    #[must_use]
    pub fn image_model(mut self, middleware: Arc<dyn ImageModelMiddleware>) -> Self {
        self.image_model.push(middleware);
        self
    }

    /// Returns `true` when no middleware is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.language_model.is_empty()
            && self.embedding_model.is_empty()
            && self.image_model.is_empty()
    }
}

/// Wraps every language, embedding and image model resolved through
/// `provider` with the matching `middleware`. Other model kinds, the provider
/// id and the services are delegated unchanged. An empty set returns
/// `provider` itself.
#[must_use]
pub fn wrap_provider(provider: ProviderRef, middleware: ProviderMiddleware) -> ProviderRef {
    if middleware.is_empty() {
        return provider;
    }
    Arc::new(WrappedProvider {
        inner: provider,
        middleware,
    })
}

struct WrappedProvider {
    inner: ProviderRef,
    middleware: ProviderMiddleware,
}

impl fmt::Debug for WrappedProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrappedProvider")
            .field("provider", self.inner.provider_id())
            .field("middleware", &self.middleware)
            .finish()
    }
}

impl WrappedProvider {
    /// Unwraps a resolved reference; providers hand out instances, so an id
    /// form cannot be wrapped and is reported as `NoSuchModel`.
    fn resolved<D: ?Sized>(
        &self,
        model: ModelRef<D>,
        model_id: &str,
        kind: ModelKind,
    ) -> Result<Arc<D>, NoSuchModelError> {
        model.into_model().map_err(|id| {
            NoSuchModelError::new(model_id, kind)
                .with_provider(self.inner.provider_id())
                .with_message(format!(
                    "provider returned the unresolved model id `{id}`, which middleware cannot wrap"
                ))
        })
    }
}

impl Provider for WrappedProvider {
    fn provider_id(&self) -> &ProviderId {
        self.inner.provider_id()
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        let model = self.inner.language_model(model_id)?;
        if self.middleware.language_model.is_empty() {
            return Ok(model);
        }
        let inner = self.resolved(model, model_id, ModelKind::Language)?;
        Ok(LanguageModelRef::from_arc(wrap_language_model(
            inner,
            self.middleware.language_model.iter().cloned(),
        )))
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        let model = self.inner.embedding_model(model_id)?;
        if self.middleware.embedding_model.is_empty() {
            return Ok(model);
        }
        let inner = self.resolved(model, model_id, ModelKind::Embedding)?;
        Ok(EmbeddingModelRef::from_arc(wrap_embedding_model(
            inner,
            self.middleware.embedding_model.iter().cloned(),
        )))
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        let model = self.inner.image_model(model_id)?;
        if self.middleware.image_model.is_empty() {
            return Ok(model);
        }
        let inner = self.resolved(model, model_id, ModelKind::Image)?;
        Ok(ImageModelRef::from_arc(wrap_image_model(
            inner,
            self.middleware.image_model.iter().cloned(),
        )))
    }

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        self.inner.transcription_model(model_id)
    }

    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        self.inner.speech_model(model_id)
    }

    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> {
        self.inner.reranking_model(model_id)
    }

    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> {
        self.inner.video_model(model_id)
    }

    fn speech_translation_model(
        &self,
        model_id: &str,
    ) -> Result<SpeechTranslationModelRef, NoSuchModelError> {
        self.inner.speech_translation_model(model_id)
    }

    fn realtime(&self) -> Option<RealtimeFactoryRef> {
        self.inner.realtime()
    }

    fn files(&self) -> Option<FilesRef> {
        self.inner.files()
    }

    fn skills(&self) -> Option<SkillsRef> {
        self.inner.skills()
    }

    fn batch(&self) -> Option<BatchRef> {
        self.inner.batch()
    }
}
