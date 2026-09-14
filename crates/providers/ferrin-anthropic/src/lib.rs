//! Ferrin provider for Anthropic.
//!
//! Implements the `ferrin-spec` traits for the Anthropic Messages API
//! (`POST /messages`, non-streaming and streaming), the Files API (uploads),
//! the Skills API and the Message Batches API, plus factories for the
//! Anthropic provider-defined and provider-executed tools.
//!
//! # Examples
//!
//! ```no_run
//! use ferrin_anthropic::AnthropicSettings;
//! use ferrin_anthropic::create_anthropic;
//! use ferrin_spec::LanguageModel;
//! use ferrin_spec::language_model::CallOptions;
//! use ferrin_spec::language_model::PromptMessage;
//!
//! # async fn run() -> Result<(), ferrin_spec::error::ProviderError> {
//! let anthropic = create_anthropic(AnthropicSettings::default())?;
//! let model = anthropic.messages("claude-sonnet-4-5");
//! let result = model
//!     .do_generate(CallOptions::new(vec![PromptMessage::user_text("Hello")]))
//!     .await?;
//! println!("{:?}", result.content);
//! # Ok(())
//! # }
//! ```
//!
//! Capability matrix and provider options: `docs/providers/anthropic.md`.

pub mod api_types;
pub mod batch;
pub mod cache_control;
pub mod capabilities;
pub mod config;
pub mod convert_prompt;
pub mod error;
pub mod files;
pub mod json_schema;
pub mod messages;
pub mod options;
pub mod output;
pub(crate) mod path;
pub mod prepare_tools;
pub mod request;
pub mod skills;
pub mod stream;
pub mod tools;
pub mod usage;

use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::settings::load_optional_setting;
use ferrin_spec::BatchRef;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::FilesRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ProviderId;
use ferrin_spec::SkillsRef;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use secrecy::SecretString;
use url::Url;

pub use crate::batch::AnthropicBatch;
pub use crate::config::AnthropicConfig;
pub use crate::config::Credential;
pub use crate::config::SharedConfig;
pub use crate::files::AnthropicFiles;
pub use crate::messages::AnthropicMessagesLanguageModel;
pub use crate::skills::AnthropicSkills;
pub use crate::tools::AnthropicTools;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings of [`create_anthropic`].
#[derive(Default)]
pub struct AnthropicSettings {
    /// Base URL; defaults to `ANTHROPIC_BASE_URL` or
    /// `https://api.anthropic.com/v1`. A bare origin gets `/v1` appended.
    pub base_url: Option<Url>,
    /// API key sent as `x-api-key`; defaults to `ANTHROPIC_API_KEY`, read on
    /// the first request.
    pub api_key: Option<SecretString>,
    /// Bearer token sent as `authorization`; defaults to
    /// `ANTHROPIC_AUTH_TOKEN` when no API key is available.
    pub auth_token: Option<SecretString>,
    /// Extra headers for every request (an `anthropic-beta` value is merged
    /// with the betas each request needs).
    pub headers: Headers,
    /// Provider name used in provider ids (default `anthropic`).
    pub name: Option<String>,
    /// HTTP transport (default: the shared `reqwest` transport).
    pub transport: Option<SharedTransport>,
    /// Generator for synthetic ids (citation sources).
    pub id_generator: Option<Arc<dyn IdGenerator>>,
}

impl std::fmt::Debug for AnthropicSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicSettings")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("auth_token", &self.auth_token.as_ref().map(|_| "***"))
            .field("headers", &self.headers)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Creates an Anthropic provider.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] when both `api_key` and
/// `auth_token` are set or when the base URL (settings or
/// `ANTHROPIC_BASE_URL`) is invalid, and [`ProviderError::Other`] when the
/// default HTTP transport cannot be built. A missing credential is reported
/// by the first request, not here.
pub fn create_anthropic(settings: AnthropicSettings) -> Result<AnthropicProvider, ProviderError> {
    if settings.api_key.is_some() && settings.auth_token.is_some() {
        return Err(InvalidArgumentError::new(
            "api_key",
            "api_key and auth_token cannot both be set; use one credential",
        )
        .into());
    }
    let base_url = match settings.base_url {
        Some(url) => config::normalize_base_url(url.as_str())?,
        None => {
            let from_env = load_optional_setting(None, config::BASE_URL_ENV);
            config::normalize_base_url(from_env.as_deref().unwrap_or(config::DEFAULT_BASE_URL))?
        }
    };
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    let mut config = AnthropicConfig::with_transport(
        settings.name.unwrap_or_else(|| "anthropic".to_owned()),
        base_url,
        transport,
    );
    config.credential = settings
        .api_key
        .map(Credential::ApiKey)
        .or(settings.auth_token.map(Credential::AuthToken));
    config.headers = settings.headers;
    if let Some(id_generator) = settings.id_generator {
        config.id_generator = id_generator;
    }
    Ok(AnthropicProvider::from_config(Arc::new(config)))
}

/// The Anthropic provider: a factory for the language model and services.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    config: SharedConfig,
    provider_id: ProviderId,
    tools: AnthropicTools,
}

impl AnthropicProvider {
    /// Creates a provider from a shared configuration.
    #[must_use]
    pub fn from_config(config: SharedConfig) -> Self {
        Self {
            provider_id: ProviderId::new(config.name.clone()),
            tools: AnthropicTools::new(),
            config,
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Messages API language model.
    #[must_use]
    pub fn messages(&self, model_id: &str) -> AnthropicMessagesLanguageModel {
        AnthropicMessagesLanguageModel::new(self.config.clone(), model_id)
    }

    /// Alias of [`Self::messages`].
    #[must_use]
    pub fn chat(&self, model_id: &str) -> AnthropicMessagesLanguageModel {
        self.messages(model_id)
    }

    /// Files service (uploads).
    #[must_use]
    pub fn files(&self) -> AnthropicFiles {
        AnthropicFiles::new(self.config.clone())
    }

    /// Skills service.
    #[must_use]
    pub fn skills(&self) -> AnthropicSkills {
        AnthropicSkills::new(self.config.clone())
    }

    /// Message batches service.
    #[must_use]
    pub fn batch(&self) -> AnthropicBatch {
        AnthropicBatch::new(self.config.clone())
    }

    /// Provider-defined and provider-executed tool factories.
    #[must_use]
    pub fn tools(&self) -> &AnthropicTools {
        &self.tools
    }
}

impl Provider for AnthropicProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Ok(self.messages(model_id).into())
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            &self.provider_id,
            model_id,
            ModelKind::Embedding,
        ))
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            &self.provider_id,
            model_id,
            ModelKind::Image,
        ))
    }

    fn files(&self) -> Option<FilesRef> {
        Some(AnthropicProvider::files(self).into())
    }

    fn skills(&self) -> Option<SkillsRef> {
        Some(AnthropicProvider::skills(self).into())
    }

    fn batch(&self) -> Option<BatchRef> {
        Some(AnthropicProvider::batch(self).into())
    }
}
