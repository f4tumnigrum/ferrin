//! Ferrin provider for OpenAI.
//!
//! Covers the Responses API (default language model), Chat Completions,
//! legacy Completions, embeddings, images, speech, transcription, files,
//! skills, batch processing, realtime sessions and, behind the `realtime`
//! feature, the WebSocket-backed transcription and speech translation
//! streams.
//!
//! ```no_run
//! use ferrin_openai::OpenAiSettings;
//! use ferrin_openai::create_openai;
//!
//! # fn main() -> Result<(), ferrin_spec::error::ProviderError> {
//! let provider = create_openai(OpenAiSettings::default())?;
//! let model = provider.responses("gpt-5");
//! # let _ = model;
//! # Ok(())
//! # }
//! ```
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the `NOTICE` file in the crate root.

pub mod batch;
pub mod capabilities;
pub mod chat;
pub mod completion;
pub mod config;
pub mod embedding;
pub mod error;
pub mod files;
pub mod image;
pub mod json_schema;
mod path;
pub mod realtime;
#[cfg(feature = "realtime")]
pub(crate) mod realtime_ws;
pub mod responses;
pub mod skills;
pub mod speech;
#[cfg(feature = "realtime")]
pub mod speech_translation;
pub(crate) mod stream_util;
pub mod tools;
pub mod transcription;

use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::parse_base_url;
use ferrin_provider_util::settings::load_optional_setting;
use ferrin_spec::BatchRef;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::FilesRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ProviderId;
use ferrin_spec::RealtimeFactoryRef;
use ferrin_spec::SkillsRef;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::TranscriptionModelRef;
#[cfg(not(feature = "realtime"))]
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use secrecy::SecretString;
use url::Url;

pub use crate::batch::OpenAiBatch;
pub use crate::chat::OpenAiChatLanguageModel;
pub use crate::completion::OpenAiCompletionLanguageModel;
pub use crate::config::OpenAiConfig;
pub use crate::config::SharedConfig;
pub use crate::embedding::OpenAiEmbeddingModel;
pub use crate::files::OpenAiFiles;
pub use crate::image::OpenAiImageModel;
pub use crate::realtime::OpenAiRealtimeFactory;
pub use crate::realtime::OpenAiRealtimeModel;
pub use crate::responses::OpenAiResponsesLanguageModel;
pub use crate::skills::OpenAiSkills;
pub use crate::speech::OpenAiSpeechModel;
#[cfg(feature = "realtime")]
pub use crate::speech_translation::OpenAiSpeechTranslationModel;
pub use crate::tools::OpenAiTools;
pub use crate::transcription::OpenAiTranscriptionModel;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings of [`create_openai`].
#[derive(Default)]
pub struct OpenAiSettings {
    /// Base URL; defaults to `OPENAI_BASE_URL` or `https://api.openai.com/v1`.
    pub base_url: Option<Url>,
    /// API key; defaults to `OPENAI_API_KEY`, read on the first request.
    pub api_key: Option<SecretString>,
    /// `OpenAI-Organization` header.
    pub organization: Option<String>,
    /// `OpenAI-Project` header.
    pub project: Option<String>,
    /// Extra headers for every request.
    pub headers: Headers,
    /// Provider name used in provider ids (default `openai`).
    pub name: Option<String>,
    /// HTTP transport (default: the shared `reqwest` transport).
    pub transport: Option<SharedTransport>,
    /// Generator for synthetic ids.
    pub id_generator: Option<Arc<dyn IdGenerator>>,
}

impl std::fmt::Debug for OpenAiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiSettings")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("organization", &self.organization)
            .field("project", &self.project)
            .field("headers", &self.headers)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Creates an OpenAI provider.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] when the base URL (settings or
/// `OPENAI_BASE_URL`) is invalid and [`ProviderError::Other`] when the
/// default HTTP transport cannot be built. A missing API key is reported by
/// the first request, not here.
pub fn create_openai(settings: OpenAiSettings) -> Result<OpenAiProvider, ProviderError> {
    let base_url = match settings.base_url {
        Some(url) => parse_base_url(url.as_str())?,
        None => {
            let from_env = load_optional_setting(None, config::BASE_URL_ENV);
            parse_base_url(from_env.as_deref().unwrap_or(config::DEFAULT_BASE_URL))?
        }
    };
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    let mut config = OpenAiConfig::with_transport(
        settings.name.unwrap_or_else(|| "openai".to_owned()),
        base_url,
        transport,
    );
    config.api_key = settings.api_key;
    config.organization = settings.organization;
    config.project = settings.project;
    config.headers = settings.headers;
    if let Some(id_generator) = settings.id_generator {
        config.id_generator = id_generator;
    }
    Ok(OpenAiProvider::from_config(Arc::new(config)))
}

/// The OpenAI provider: a factory for every model and service.
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    config: SharedConfig,
    provider_id: ProviderId,
    tools: OpenAiTools,
}

impl OpenAiProvider {
    /// Creates a provider from a shared configuration.
    #[must_use]
    pub fn from_config(config: SharedConfig) -> Self {
        Self {
            provider_id: ProviderId::new(config.name.clone()),
            tools: OpenAiTools::new(),
            config,
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Responses API language model.
    #[must_use]
    pub fn responses(&self, model_id: &str) -> OpenAiResponsesLanguageModel {
        OpenAiResponsesLanguageModel::new(self.config.clone(), model_id)
    }

    /// Chat Completions language model.
    #[must_use]
    pub fn chat(&self, model_id: &str) -> OpenAiChatLanguageModel {
        OpenAiChatLanguageModel::new(self.config.clone(), model_id)
    }

    /// Legacy Completions language model.
    #[must_use]
    pub fn completion(&self, model_id: &str) -> OpenAiCompletionLanguageModel {
        OpenAiCompletionLanguageModel::new(self.config.clone(), model_id)
    }

    /// Embedding model.
    #[must_use]
    pub fn embedding(&self, model_id: &str) -> OpenAiEmbeddingModel {
        OpenAiEmbeddingModel::new(self.config.clone(), model_id)
    }

    /// Image model.
    #[must_use]
    pub fn image(&self, model_id: &str) -> OpenAiImageModel {
        OpenAiImageModel::new(self.config.clone(), model_id)
    }

    /// Speech model.
    #[must_use]
    pub fn speech(&self, model_id: &str) -> OpenAiSpeechModel {
        OpenAiSpeechModel::new(self.config.clone(), model_id)
    }

    /// Transcription model.
    #[must_use]
    pub fn transcription(&self, model_id: &str) -> OpenAiTranscriptionModel {
        OpenAiTranscriptionModel::new(self.config.clone(), model_id)
    }

    /// Speech translation model (WebSocket; requires the `realtime` feature).
    #[cfg(feature = "realtime")]
    #[must_use]
    pub fn speech_translation(&self, model_id: &str) -> OpenAiSpeechTranslationModel {
        OpenAiSpeechTranslationModel::new(self.config.clone(), model_id)
    }

    /// Files service.
    #[must_use]
    pub fn files(&self) -> OpenAiFiles {
        OpenAiFiles::new(self.config.clone())
    }

    /// Skills service.
    #[must_use]
    pub fn skills(&self) -> OpenAiSkills {
        OpenAiSkills::new(self.config.clone())
    }

    /// Batch service.
    #[must_use]
    pub fn batch(&self) -> OpenAiBatch {
        OpenAiBatch::new(self.config.clone())
    }

    /// Realtime model factory.
    #[must_use]
    pub fn realtime(&self) -> OpenAiRealtimeFactory {
        OpenAiRealtimeFactory::new(self.config.clone())
    }

    /// Provider-defined and provider-executed tool factories.
    #[must_use]
    pub fn tools(&self) -> &OpenAiTools {
        &self.tools
    }
}

impl Provider for OpenAiProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Ok(self.responses(model_id).into())
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        Ok(self.embedding(model_id).into())
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        Ok(self.image(model_id).into())
    }

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        Ok(self.transcription(model_id).into())
    }

    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        Ok(self.speech(model_id).into())
    }

    fn speech_translation_model(
        &self,
        model_id: &str,
    ) -> Result<SpeechTranslationModelRef, NoSuchModelError> {
        #[cfg(feature = "realtime")]
        {
            Ok(self.speech_translation(model_id).into())
        }
        #[cfg(not(feature = "realtime"))]
        {
            Err(NoSuchModelError::unsupported_kind(
                &self.provider_id,
                model_id,
                ModelKind::SpeechTranslation,
            )
            .with_message("enable the `realtime` feature of ferrin-openai for speech translation"))
        }
    }

    fn realtime(&self) -> Option<RealtimeFactoryRef> {
        Some(OpenAiProvider::realtime(self).into())
    }

    fn files(&self) -> Option<FilesRef> {
        Some(OpenAiProvider::files(self).into())
    }

    fn skills(&self) -> Option<SkillsRef> {
        Some(OpenAiProvider::skills(self).into())
    }

    fn batch(&self) -> Option<BatchRef> {
        Some(OpenAiProvider::batch(self).into())
    }
}
