//! Azure connection settings and credential callbacks.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use ferrin_provider_util::SharedTransport;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::error::ProviderError;
use secrecy::SecretString;
use url::Url;

/// Returns a Microsoft Entra access token for requests without an Authorization override.
pub trait TokenProvider: Send + Sync + 'static {
    /// Acquires an access token.
    ///
    /// # Errors
    ///
    /// Returns the credential backend error; Azure transport redacts its details.
    fn token(&self) -> BoxFuture<'_, Result<SecretString, ProviderError>>;
}

struct TokenProviderFn<F>(F);

impl<F, Fut> TokenProvider for TokenProviderFn<F>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<SecretString, ProviderError>> + Send + 'static,
{
    fn token(&self) -> BoxFuture<'_, Result<SecretString, ProviderError>> {
        Box::pin((self.0)())
    }
}

/// Adapts an asynchronous token callback.
pub fn token_provider<F, Fut>(callback: F) -> Arc<dyn TokenProvider>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<SecretString, ProviderError>> + Send + 'static,
{
    Arc::new(TokenProviderFn(callback))
}

/// Azure endpoint layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AzureUrlMode {
    /// Azure hosts use `/v1`; complete v1 and custom gateway URLs are preserved.
    #[default]
    V1,
    /// Use `/deployments/{deployment}` and the `api-version` query parameter.
    Deployment,
}

/// Settings for [`crate::create_azure`].
#[derive(Default)]
pub struct AzureSettings {
    /// Resource name, falling back to `AZURE_RESOURCE_NAME` without a base URL.
    pub resource_name: Option<String>,
    /// Explicit API base URL, including `/openai` or `/openai/v1` as appropriate.
    pub base_url: Option<Url>,
    /// API key, loaded from `AZURE_API_KEY` on each request when absent.
    pub api_key: Option<SecretString>,
    /// Entra authentication, mutually exclusive with `api_key`; an explicit
    /// Authorization header bypasses the callback for that request.
    pub token_provider: Option<Arc<dyn TokenProvider>>,
    /// Additional request headers; credentials use the dedicated fields above.
    pub headers: Headers,
    /// URL layout (v1 by default).
    pub url_mode: AzureUrlMode,
    /// Azure API version for endpoints requiring a version query (default `v1`).
    pub api_version: Option<String>,
    /// Shared HTTP transport override.
    pub transport: Option<SharedTransport>,
}

impl AzureSettings {
    /// Selects an Azure OpenAI resource.
    #[must_use]
    pub fn new(resource_name: impl Into<String>) -> Self {
        Self {
            resource_name: Some(resource_name.into()),
            ..Self::default()
        }
    }
}

impl fmt::Debug for AzureSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureSettings")
            .field("resource_name", &self.resource_name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("token_provider", &self.token_provider.is_some())
            .field("headers", &self.headers)
            .field("url_mode", &self.url_mode)
            .field("api_version", &self.api_version)
            .finish_non_exhaustive()
    }
}
