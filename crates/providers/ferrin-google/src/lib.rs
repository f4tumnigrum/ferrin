//! Ferrin provider for Google Generative AI (Gemini API).
//!
//! Implements the `ferrin-spec` traits for `generateContent` (non-streaming
//! and streaming), embeddings, Gemini image generation, speech (TTS),
//! transcription, Veo video generation, the Files API, batch generation and
//! Live API sessions, plus factories for the Google provider-executed tools.
//!
//! # Examples
//!
//! ```no_run
//! use ferrin_google::GoogleSettings;
//! use ferrin_google::create_google;
//! use ferrin_spec::LanguageModel;
//! use ferrin_spec::language_model::CallOptions;
//! use ferrin_spec::language_model::PromptMessage;
//!
//! # async fn run() -> Result<(), ferrin_spec::error::ProviderError> {
//! let google = create_google(GoogleSettings::default())?;
//! let model = google.language_model("gemini-2.5-flash");
//! let result = model
//!     .do_generate(CallOptions::new(vec![PromptMessage::user_text("Hello")]))
//!     .await?;
//! println!("{:?}", result.content);
//! # Ok(())
//! # }
//! ```
//!
//! Capability matrix and provider options: `docs/providers/google.md`.

pub mod api_types;
pub mod batch;
pub mod capabilities;
pub mod config;
pub mod convert_prompt;
pub mod embedding;
pub mod error;
pub mod files;
pub mod image;
pub mod json_accumulator;
pub mod json_schema;
pub mod language_model;
pub mod options;
pub mod output;
pub mod prepare_tools;
pub mod realtime;
pub mod request;
pub mod speech;
pub mod stream;
pub mod tools;
pub mod transcription;
pub mod video;

use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_spec::BatchRef;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::FilesRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ProviderId;
use ferrin_spec::RealtimeFactoryRef;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::VideoModelRef;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use secrecy::SecretString;
use url::Url;

pub use crate::batch::GoogleBatch;
pub use crate::config::GoogleConfig;
pub use crate::config::SharedConfig;
pub use crate::embedding::GoogleEmbeddingModel;
pub use crate::files::GoogleFiles;
pub use crate::image::GoogleImageModel;
pub use crate::language_model::GoogleLanguageModel;
pub use crate::realtime::GoogleRealtimeFactory;
pub use crate::realtime::GoogleRealtimeModel;
pub use crate::speech::GoogleSpeechModel;
pub use crate::tools::GoogleTools;
pub use crate::transcription::GoogleTranscriptionModel;
pub use crate::video::GoogleVideoModel;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings of [`create_google`].
#[derive(Default)]
pub struct GoogleSettings {
    /// Base URL; defaults to `https://generativelanguage.googleapis.com/v1beta`.
    pub base_url: Option<Url>,
    /// API key sent as `x-goog-api-key`; defaults to
    /// `GOOGLE_GENERATIVE_AI_API_KEY`, read on the first request.
    pub api_key: Option<SecretString>,
    /// Extra headers for every request.
    pub headers: Headers,
    /// Provider name used in provider ids and as the additional option key
    /// (default `google`).
    pub name: Option<String>,
    /// HTTP transport (default: the shared `reqwest` transport).
    pub transport: Option<SharedTransport>,
    /// Generator for synthetic ids (tool calls without an id, sources).
    pub id_generator: Option<Arc<dyn IdGenerator>>,
}

impl std::fmt::Debug for GoogleSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleSettings")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("headers", &self.headers)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Creates a Google Generative AI provider.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] when the base URL is invalid
/// and [`ProviderError::Other`] when the default HTTP transport cannot be
/// built. A missing API key is reported by the first request, not here.
pub fn create_google(settings: GoogleSettings) -> Result<GoogleProvider, ProviderError> {
    let base_url = match settings.base_url {
        Some(url) => ferrin_provider_util::base_url::parse_base_url(url.as_str())?,
        None => ferrin_provider_util::base_url::parse_base_url(config::DEFAULT_BASE_URL)?,
    };
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    let mut config = GoogleConfig::with_transport(
        settings
            .name
            .unwrap_or_else(|| config::DEFAULT_NAME.to_owned()),
        base_url,
        transport,
    );
    config.api_key = settings.api_key;
    config.headers = settings.headers;
    if let Some(id_generator) = settings.id_generator {
        config.id_generator = id_generator;
    }
    Ok(GoogleProvider::from_config(Arc::new(config)))
}

/// The Google provider: a factory for the models and services.
#[derive(Debug, Clone)]
pub struct GoogleProvider {
    config: SharedConfig,
    provider_id: ProviderId,
    tools: GoogleTools,
}

impl GoogleProvider {
    /// Creates a provider from a shared configuration.
    #[must_use]
    pub fn from_config(config: SharedConfig) -> Self {
        Self {
            provider_id: ProviderId::new(config.name.clone()),
            tools: GoogleTools::new(),
            config,
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Gemini language model (`generateContent`).
    #[must_use]
    pub fn language_model(&self, model_id: &str) -> GoogleLanguageModel {
        GoogleLanguageModel::new(self.config.clone(), model_id)
    }

    /// Alias of [`Self::language_model`].
    #[must_use]
    pub fn chat(&self, model_id: &str) -> GoogleLanguageModel {
        self.language_model(model_id)
    }

    /// Embedding model (`embedContent` / `batchEmbedContents`).
    #[must_use]
    pub fn embedding(&self, model_id: &str) -> GoogleEmbeddingModel {
        GoogleEmbeddingModel::new(self.config.clone(), model_id)
    }

    /// Alias of [`Self::embedding`].
    #[must_use]
    pub fn text_embedding(&self, model_id: &str) -> GoogleEmbeddingModel {
        self.embedding(model_id)
    }

    /// Gemini image model (`generateContent` with the `IMAGE` modality).
    #[must_use]
    pub fn image(&self, model_id: &str) -> GoogleImageModel {
        GoogleImageModel::new(self.config.clone(), model_id)
    }

    /// Speech (text-to-speech) model.
    #[must_use]
    pub fn speech(&self, model_id: &str) -> GoogleSpeechModel {
        GoogleSpeechModel::new(self.config.clone(), model_id)
    }

    /// Transcription model (Interactions API).
    #[must_use]
    pub fn transcription(&self, model_id: &str) -> GoogleTranscriptionModel {
        GoogleTranscriptionModel::new(self.config.clone(), model_id)
    }

    /// Veo video model.
    #[must_use]
    pub fn video(&self, model_id: &str) -> GoogleVideoModel {
        GoogleVideoModel::new(self.config.clone(), model_id)
    }

    /// Files API service.
    #[must_use]
    pub fn files(&self) -> GoogleFiles {
        GoogleFiles::new(self.config.clone())
    }

    /// Batch generation service.
    #[must_use]
    pub fn batch(&self) -> GoogleBatch {
        GoogleBatch::new(self.config.clone())
    }

    /// Live API factory (session tokens and event mapping).
    #[must_use]
    pub fn realtime(&self) -> GoogleRealtimeFactory {
        GoogleRealtimeFactory::new(self.config.clone())
    }

    /// Provider-executed tool factories.
    #[must_use]
    pub fn tools(&self) -> &GoogleTools {
        &self.tools
    }
}

impl Provider for GoogleProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Ok(GoogleProvider::language_model(self, model_id).into())
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

    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> {
        Ok(self.video(model_id).into())
    }

    fn realtime(&self) -> Option<RealtimeFactoryRef> {
        Some(GoogleProvider::realtime(self).into())
    }

    fn files(&self) -> Option<FilesRef> {
        Some(GoogleProvider::files(self).into())
    }

    fn batch(&self) -> Option<BatchRef> {
        Some(GoogleProvider::batch(self).into())
    }
}
