//! Ferrin provider for OpenAI-compatible endpoints.
//!
//! A configurable adapter for services that expose OpenAI-shaped Chat
//! Completions, Completions, embeddings and image endpoints. It is meant to
//! be used directly (any endpoint, any model id) or as the building block of
//! a dedicated provider crate, which supplies its own name, error body
//! structure, metadata extractor and request body transformer.
//!
//! # Examples
//!
//! ```no_run
//! use ferrin_openai_compatible::OpenAiCompatibleSettings;
//! use ferrin_openai_compatible::create_openai_compatible;
//! use ferrin_spec::LanguageModel;
//! use ferrin_spec::language_model::CallOptions;
//! use ferrin_spec::language_model::PromptMessage;
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut settings = OpenAiCompatibleSettings::new(
//!     "example",
//!     Url::parse("https://api.example.com/v1")?,
//! );
//! settings.api_key_env = Some("EXAMPLE_API_KEY".to_owned());
//! let provider = create_openai_compatible(settings)?;
//! let model = provider.chat("example-model");
//! let result = model
//!     .do_generate(CallOptions::new(vec![PromptMessage::user_text("Hello")]))
//!     .await?;
//! println!("{:?}", result.content);
//! # Ok(())
//! # }
//! ```
//!
//! Capability matrix and provider options: `docs/providers/openai-compatible.md`.

pub mod chat;
pub mod completion;
pub mod config;
pub mod embedding;
pub mod error;
pub mod image;
pub mod metadata;
pub mod options_key;

use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::without_trailing_slash;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ProviderId;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::provider::Provider;
use secrecy::SecretString;
use url::Url;

pub use crate::chat::OpenAiCompatibleChatLanguageModel;
pub use crate::completion::OpenAiCompatibleCompletionLanguageModel;
pub use crate::config::ConvertUsage;
pub use crate::config::OpenAiCompatibleConfig;
pub use crate::config::SharedConfig;
pub use crate::config::TransformRequestBody;
pub use crate::embedding::OpenAiCompatibleEmbeddingModel;
pub use crate::error::DefaultErrorStructure;
pub use crate::error::ErrorStructure;
pub use crate::error::SharedErrorStructure;
pub use crate::image::OpenAiCompatibleImageModel;
pub use crate::metadata::MetadataExtractor;
pub use crate::metadata::SharedMetadataExtractor;
pub use crate::metadata::StreamMetadataExtractor;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings of [`create_openai_compatible`].
pub struct OpenAiCompatibleSettings {
    /// Provider name: prefix of every provider id and provider options key.
    pub name: String,
    /// Base URL of the API (`https://api.example.com/v1`); a trailing slash
    /// is removed.
    pub base_url: Url,
    /// API key sent as `authorization: Bearer <key>`.
    pub api_key: Option<SecretString>,
    /// Environment variable read for the API key on each request when
    /// `api_key` is `None`.
    pub api_key_env: Option<String>,
    /// Extra headers for every request.
    pub headers: Headers,
    /// Query parameters appended to every request URL.
    pub query_params: Vec<(String, String)>,
    /// Ask for usage in streaming responses (`stream_options.include_usage`).
    pub include_usage: bool,
    /// Whether the chat model supports `response_format.json_schema`.
    pub supports_structured_outputs: bool,
    /// URLs the chat model accepts as file parts (default: none).
    pub supported_urls: Option<SupportedUrls>,
    /// HTTP transport (default: the shared `reqwest` transport).
    pub transport: Option<SharedTransport>,
    /// Generator for synthetic ids.
    pub id_generator: Option<Arc<dyn IdGenerator>>,
    /// Error body structure (default: the OpenAI error body).
    pub error_structure: Option<SharedErrorStructure>,
    /// Metadata extractor of the chat model.
    pub metadata_extractor: Option<SharedMetadataExtractor>,
    /// Request body transformer of the chat model.
    pub transform_request_body: Option<TransformRequestBody>,
    /// Usage converter of the chat model.
    pub convert_usage: Option<ConvertUsage>,
    /// Maximum values per embedding call (default 2048).
    pub max_embeddings_per_call: Option<usize>,
    /// Whether embedding calls may run in parallel (default `true`).
    pub supports_parallel_calls: Option<bool>,
}

impl OpenAiCompatibleSettings {
    /// Settings for `name` at `base_url` with every option at its default.
    #[must_use]
    pub fn new(name: impl Into<String>, base_url: Url) -> Self {
        Self {
            name: name.into(),
            base_url,
            api_key: None,
            api_key_env: None,
            headers: Headers::new(),
            query_params: Vec::new(),
            include_usage: false,
            supports_structured_outputs: false,
            supported_urls: None,
            transport: None,
            id_generator: None,
            error_structure: None,
            metadata_extractor: None,
            transform_request_body: None,
            convert_usage: None,
            max_embeddings_per_call: None,
            supports_parallel_calls: None,
        }
    }
}

impl std::fmt::Debug for OpenAiCompatibleSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleSettings")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_key_env", &self.api_key_env)
            .field("headers", &self.headers)
            .field("query_params", &self.query_params)
            .field("include_usage", &self.include_usage)
            .field(
                "supports_structured_outputs",
                &self.supports_structured_outputs,
            )
            .finish_non_exhaustive()
    }
}

/// Creates an OpenAI-compatible provider.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] when `name` is empty or
/// contains a `.`, and [`ProviderError::Other`] when the default HTTP
/// transport cannot be built.
pub fn create_openai_compatible(
    settings: OpenAiCompatibleSettings,
) -> Result<OpenAiCompatibleProvider, ProviderError> {
    let name = settings.name.trim();
    if name.is_empty() || name.contains('.') {
        return Err(InvalidArgumentError::new(
            "name",
            "name must be a non-empty provider name without `.`",
        )
        .into());
    }
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    let mut config = OpenAiCompatibleConfig::with_transport(
        name,
        without_trailing_slash(settings.base_url),
        transport,
    );
    config.api_key = settings.api_key;
    config.api_key_env = settings.api_key_env;
    config.headers = settings.headers;
    config.query_params = settings.query_params;
    config.include_usage = settings.include_usage;
    config.supports_structured_outputs = settings.supports_structured_outputs;
    if let Some(urls) = settings.supported_urls {
        config.supported_urls = urls;
    }
    if let Some(id_generator) = settings.id_generator {
        config.id_generator = id_generator;
    }
    if let Some(structure) = settings.error_structure {
        config.error_structure = structure;
    }
    config.metadata_extractor = settings.metadata_extractor;
    config.transform_request_body = settings.transform_request_body;
    config.convert_usage = settings.convert_usage;
    if let Some(max) = settings.max_embeddings_per_call {
        config.max_embeddings_per_call = max;
    }
    if let Some(parallel) = settings.supports_parallel_calls {
        config.supports_parallel_calls = parallel;
    }
    Ok(OpenAiCompatibleProvider::from_config(Arc::new(config)))
}

/// An OpenAI-compatible provider: a factory for its models.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    config: SharedConfig,
    provider_id: ProviderId,
}

impl OpenAiCompatibleProvider {
    /// Creates a provider from a shared configuration.
    #[must_use]
    pub fn from_config(config: SharedConfig) -> Self {
        Self {
            provider_id: ProviderId::new(config.name.clone()),
            config,
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Chat Completions language model.
    #[must_use]
    pub fn chat(&self, model_id: &str) -> OpenAiCompatibleChatLanguageModel {
        OpenAiCompatibleChatLanguageModel::new(self.config.clone(), model_id)
    }

    /// Legacy Completions language model.
    #[must_use]
    pub fn completion(&self, model_id: &str) -> OpenAiCompatibleCompletionLanguageModel {
        OpenAiCompatibleCompletionLanguageModel::new(self.config.clone(), model_id)
    }

    /// Embedding model.
    #[must_use]
    pub fn embedding(&self, model_id: &str) -> OpenAiCompatibleEmbeddingModel {
        OpenAiCompatibleEmbeddingModel::new(self.config.clone(), model_id)
    }

    /// Image model.
    #[must_use]
    pub fn image(&self, model_id: &str) -> OpenAiCompatibleImageModel {
        OpenAiCompatibleImageModel::new(self.config.clone(), model_id)
    }
}

impl Provider for OpenAiCompatibleProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Ok(self.chat(model_id).into())
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        Ok(self.embedding(model_id).into())
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        Ok(self.image(model_id).into())
    }
}
