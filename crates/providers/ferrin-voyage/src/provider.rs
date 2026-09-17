//! Voyage provider factory.
//!
//! Derived from Vercel AI SDK `packages/voyage/src/voyage-provider.ts`
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); translated and modified.

use std::sync::Arc;

use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::parse_base_url;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ModelId;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::RerankingModelRef;
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use secrecy::SecretString;
use url::Url;

use crate::config::DEFAULT_BASE_URL;
use crate::config::SharedConfig;
use crate::config::VoyageConfig;
use crate::reranking::VoyageRerankingModel;

/// Settings for [`create_voyage`].
#[derive(Default)]
pub struct VoyageSettings {
    /// API base URL; defaults to `https://api.voyageai.com/v1`.
    pub base_url: Option<Url>,
    /// API key; when absent, requests read `VOYAGE_API_KEY` lazily.
    pub api_key: Option<SecretString>,
    /// Provider name; defaults to `voyage`.
    pub name: Option<String>,
    /// Extra HTTP headers.
    pub headers: Headers,
    /// Custom HTTP transport; defaults to the shared reqwest transport.
    pub transport: Option<SharedTransport>,
}

impl std::fmt::Debug for VoyageSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoyageSettings")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("name", &self.name)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

/// Creates a Voyage provider, resolving credentials only when making a request.
///
/// # Errors
///
/// Returns an error for an invalid provider name, invalid base URL or a
/// failure to construct the default transport.
///
/// # Examples
///
/// ```no_run
/// let provider = ferrin_voyage::create_voyage(Default::default())?;
/// let model = provider.reranking("rerank-2.5");
/// # Ok::<(), ferrin_spec::error::ProviderError>(())
/// ```
pub fn create_voyage(settings: VoyageSettings) -> Result<VoyageProvider, ProviderError> {
    let base_url = match settings.base_url {
        Some(url) => url,
        None => parse_base_url(DEFAULT_BASE_URL)?,
    };
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    let mut config = VoyageConfig::with_transport(
        settings.name.unwrap_or_else(|| "voyage".to_owned()),
        base_url,
        transport,
    )?;
    config.api_key = settings.api_key;
    config.headers = settings.headers;
    Ok(VoyageProvider::from_config(Arc::new(config)))
}

/// Factory for Voyage reranking models.
#[derive(Debug, Clone)]
pub struct VoyageProvider {
    config: SharedConfig,
    provider_id: ProviderId,
}

impl VoyageProvider {
    /// Creates a provider from shared configuration.
    #[must_use]
    pub fn from_config(config: SharedConfig) -> Self {
        Self {
            provider_id: ProviderId::new(config.name.clone()),
            config,
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Creates a concrete reranking model for any Voyage model ID.
    #[must_use]
    pub fn reranking(&self, model_id: impl Into<ModelId>) -> VoyageRerankingModel {
        VoyageRerankingModel::new(self.config.clone(), model_id)
    }
}

impl Provider for VoyageProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Language,
        ))
    }

    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Embedding,
        ))
    }

    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(
            self.provider_id(),
            model_id,
            ModelKind::Image,
        ))
    }

    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> {
        Ok(self.reranking(model_id).into())
    }
}
