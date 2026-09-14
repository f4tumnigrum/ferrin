//! Registry of providers addressed as `provider:model`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ModelKind;
use ferrin_spec::NoSuchModelError;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderRef;
use ferrin_spec::RealtimeModelRef;
use ferrin_spec::RerankingModelRef;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::VideoModelRef;

use crate::error::Error;
use crate::error::NoSuchProviderDetails;
use crate::middleware::LanguageModelMiddleware;
use crate::middleware::wrap_language_model;

/// Resolves model ids of the form `<provider><separator><model>`.
#[derive(Clone)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, ProviderRef>,
    separator: String,
    language_model_middleware: Vec<Arc<dyn LanguageModelMiddleware>>,
    id: ProviderId,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("separator", &self.separator)
            .field(
                "language_model_middleware",
                &self.language_model_middleware.len(),
            )
            .finish()
    }
}

/// Creates a registry from `(id, provider)` pairs with the default
/// separator `:`.
pub fn create_provider_registry(
    providers: impl IntoIterator<Item = (impl Into<String>, ProviderRef)>,
) -> ProviderRegistry {
    let mut builder = ProviderRegistry::builder();
    for (id, provider) in providers {
        builder = builder.provider(id, provider);
    }
    builder.build()
}

impl ProviderRegistry {
    /// Starts building a registry.
    #[must_use]
    pub fn builder() -> ProviderRegistryBuilder {
        ProviderRegistryBuilder::default()
    }

    /// The registered provider ids.
    pub fn provider_ids(&self) -> impl Iterator<Item = &str> + '_ {
        self.providers.keys().map(String::as_str)
    }

    /// Looks up a provider by id.
    #[must_use]
    pub fn provider(&self, id: &str) -> Option<&ProviderRef> {
        self.providers.get(id)
    }

    fn split<'a>(&self, id: &'a str, kind: ModelKind) -> Result<(&ProviderRef, &'a str), Error> {
        let not_found = |provider_id: &str| {
            Error::no_such_provider(NoSuchProviderDetails {
                provider_id: ProviderId::new(provider_id),
                available_providers: self.providers.keys().map(ProviderId::new).collect(),
                model_id: id.to_owned(),
                model_kind: kind,
            })
        };
        let Some((provider_id, model_id)) = id.split_once(self.separator.as_str()) else {
            return Err(not_found(id));
        };
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| not_found(provider_id))?;
        Ok((provider, model_id))
    }

    /// Resolves a language model, applying the registry middleware.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchProvider`] for unknown providers or malformed ids,
    /// [`Error::Provider`] (`NoSuchModel`) from the provider.
    pub fn language_model(&self, id: &str) -> Result<LanguageModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Language)?;
        let model = provider.language_model(model_id)?;
        if self.language_model_middleware.is_empty() {
            return Ok(model);
        }
        let inner = model
            .into_model()
            .map_err(|id| Error::NoDefaultRegistry { model_id: id })?;
        Ok(LanguageModelRef::from_arc(wrap_language_model(
            inner,
            self.language_model_middleware.iter().cloned(),
        )))
    }

    /// Resolves an embedding model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn embedding_model(&self, id: &str) -> Result<EmbeddingModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Embedding)?;
        Ok(provider.embedding_model(model_id)?)
    }

    /// Resolves an image model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn image_model(&self, id: &str) -> Result<ImageModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Image)?;
        Ok(provider.image_model(model_id)?)
    }

    /// Resolves a transcription model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn transcription_model(&self, id: &str) -> Result<TranscriptionModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Transcription)?;
        Ok(provider.transcription_model(model_id)?)
    }

    /// Resolves a speech model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn speech_model(&self, id: &str) -> Result<SpeechModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Speech)?;
        Ok(provider.speech_model(model_id)?)
    }

    /// Resolves a reranking model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn reranking_model(&self, id: &str) -> Result<RerankingModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Reranking)?;
        Ok(provider.reranking_model(model_id)?)
    }

    /// Resolves a video model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn video_model(&self, id: &str) -> Result<VideoModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Video)?;
        Ok(provider.video_model(model_id)?)
    }

    /// Resolves a speech translation model.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`].
    pub fn speech_translation_model(&self, id: &str) -> Result<SpeechTranslationModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::SpeechTranslation)?;
        Ok(provider.speech_translation_model(model_id)?)
    }

    /// Resolves a realtime model through the provider's realtime factory.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::language_model`]; providers without realtime
    /// support yield a `NoSuchModel` error.
    pub fn realtime_model(&self, id: &str) -> Result<RealtimeModelRef, Error> {
        let (provider, model_id) = self.split(id, ModelKind::Realtime)?;
        let factory = provider.realtime().ok_or_else(|| {
            Error::from(
                NoSuchModelError::new(model_id, ModelKind::Realtime)
                    .with_provider(provider.provider_id())
                    .with_message(format!(
                        "provider `{}` does not support realtime sessions",
                        provider.provider_id()
                    )),
            )
        })?;
        Ok(factory.model(model_id)?)
    }
}

fn to_no_such_model(error: Error, id: &str, kind: ModelKind) -> NoSuchModelError {
    match error {
        Error::Provider(provider) => match *provider {
            ferrin_spec::ProviderError::NoSuchModel(inner) => *inner,
            other => NoSuchModelError::new(id, kind).with_message(other.to_string()),
        },
        other => NoSuchModelError::new(id, kind).with_message(other.to_string()),
    }
}

impl Provider for ProviderRegistry {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        ProviderRegistry::language_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Language))
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        ProviderRegistry::embedding_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Embedding))
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        ProviderRegistry::image_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Image))
    }

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        ProviderRegistry::transcription_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Transcription))
    }

    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        ProviderRegistry::speech_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Speech))
    }

    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> {
        ProviderRegistry::reranking_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Reranking))
    }

    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> {
        ProviderRegistry::video_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::Video))
    }

    fn speech_translation_model(
        &self,
        model_id: &str,
    ) -> Result<SpeechTranslationModelRef, NoSuchModelError> {
        ProviderRegistry::speech_translation_model(self, model_id)
            .map_err(|error| to_no_such_model(error, model_id, ModelKind::SpeechTranslation))
    }
}

/// Builder of a [`ProviderRegistry`].
pub struct ProviderRegistryBuilder {
    providers: BTreeMap<String, ProviderRef>,
    separator: String,
    language_model_middleware: Vec<Arc<dyn LanguageModelMiddleware>>,
}

impl Default for ProviderRegistryBuilder {
    fn default() -> Self {
        Self {
            providers: BTreeMap::new(),
            separator: ":".to_owned(),
            language_model_middleware: Vec::new(),
        }
    }
}

impl fmt::Debug for ProviderRegistryBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistryBuilder")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("separator", &self.separator)
            .field(
                "language_model_middleware",
                &self.language_model_middleware.len(),
            )
            .finish()
    }
}

impl ProviderRegistryBuilder {
    /// Registers `provider` under `id` (replacing an existing entry).
    #[must_use]
    pub fn provider(mut self, id: impl Into<String>, provider: ProviderRef) -> Self {
        self.providers.insert(id.into(), provider);
        self
    }

    /// Sets the separator between provider and model id (default `:`).
    #[must_use]
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Applies `middleware` to every resolved language model.
    #[must_use]
    pub fn language_model_middleware(
        mut self,
        middleware: Arc<dyn LanguageModelMiddleware>,
    ) -> Self {
        self.language_model_middleware.push(middleware);
        self
    }

    /// Builds the registry.
    #[must_use]
    pub fn build(self) -> ProviderRegistry {
        ProviderRegistry {
            providers: self.providers,
            separator: self.separator,
            language_model_middleware: self.language_model_middleware,
            id: ProviderId::new("registry"),
        }
    }
}
