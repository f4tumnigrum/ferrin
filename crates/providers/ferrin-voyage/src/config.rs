//! Shared Voyage configuration and lazy credentials.

use std::fmt;
use std::sync::Arc;

use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::join_path;
use ferrin_provider_util::base_url::without_trailing_slash;
use ferrin_provider_util::settings::ApiKeyConfig;
use ferrin_provider_util::settings::load_api_key;
use ferrin_spec::Headers;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use url::Url;

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.voyageai.com/v1";
const USER_AGENT: &str = concat!("ferrin-voyage/", env!("CARGO_PKG_VERSION"));

/// Configuration shared by Voyage reranking models.
pub struct VoyageConfig {
    /// Provider name and additional provider-options key.
    pub name: String,
    /// Base URL, normally `https://api.voyageai.com/v1`.
    pub base_url: Url,
    /// Explicit API key; otherwise `VOYAGE_API_KEY` is read on each request.
    pub api_key: Option<SecretString>,
    /// Extra headers; per-call headers override these values.
    pub headers: Headers,
    /// HTTP transport.
    pub transport: SharedTransport,
}

impl fmt::Debug for VoyageConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VoyageConfig")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl VoyageConfig {
    /// Creates a configuration using the default HTTP transport.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument error for an invalid name or base URL,
    /// or a transport construction error.
    pub fn new(name: impl Into<String>, base_url: Url) -> Result<Self, ProviderError> {
        let transport = ferrin_provider_util::default_transport().map_err(ProviderError::other)?;
        Self::with_transport(name, base_url, transport)
    }

    /// Creates a configuration using an explicit HTTP transport.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument error for an empty/dotted name or a base
    /// URL with a non-HTTP scheme, credentials, query string or fragment.
    pub fn with_transport(
        name: impl Into<String>,
        base_url: Url,
        transport: SharedTransport,
    ) -> Result<Self, ProviderError> {
        let name = name.into();
        if name.trim().is_empty() || name.contains('.') {
            return Err(InvalidArgumentError::new(
                "name",
                "name must be non-empty and must not contain a dot",
            )
            .into());
        }
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(InvalidArgumentError::new(
                "base_url",
                "base_url must be an HTTP(S) URL without credentials, query or fragment",
            )
            .into());
        }
        Ok(Self {
            name: name.trim().to_owned(),
            base_url: without_trailing_slash(base_url),
            api_key: None,
            headers: Headers::new(),
            transport,
        })
    }

    /// Resolves an explicit API key or the `VOYAGE_API_KEY` environment variable.
    ///
    /// # Errors
    ///
    /// Returns a missing-key error when neither source supplies a key.
    pub fn api_key(&self) -> Result<SecretString, ProviderError> {
        Ok(load_api_key(ApiKeyConfig {
            api_key: self.api_key.clone(),
            environment_variable: "VOYAGE_API_KEY",
            parameter_name: "api_key",
            description: "Voyage",
        })?)
    }

    /// Resolves the API key, then applies configured and per-call header overrides.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing key or an invalid key header value.
    pub fn headers(&self, call_headers: &Headers) -> Result<Headers, ProviderError> {
        let key = self.api_key()?;
        let mut headers = self.headers.clone().merged(call_headers);
        if !headers.contains("authorization") {
            headers
                .insert("authorization", &format!("Bearer {}", key.expose_secret()))
                .map_err(|_| {
                    InvalidArgumentError::new("api_key", "api_key is not a valid header value")
                })?;
        }
        Ok(headers.with_user_agent_suffix([USER_AGENT]))
    }

    /// Returns the URL of an API path below the configured base URL.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        join_path(&self.base_url, path)
    }
}

/// Shared handle to Voyage configuration.
pub type SharedConfig = Arc<VoyageConfig>;
