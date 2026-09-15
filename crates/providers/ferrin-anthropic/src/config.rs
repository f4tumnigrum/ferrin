//! Shared configuration of every Anthropic model and service.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::PrefixedIdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::join_path;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::settings::ApiKeyConfig;
use ferrin_provider_util::settings::load_api_key;
use ferrin_spec::Headers;
use ferrin_spec::ProviderId;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use url::Url;

/// User-agent suffix appended to every request.
pub const USER_AGENT: &str = concat!("ferrin-anthropic/", env!("CARGO_PKG_VERSION"));

/// Environment variable read for the API key when none is configured.
pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// Environment variable read for the bearer token when none is configured.
pub const AUTH_TOKEN_ENV: &str = "ANTHROPIC_AUTH_TOKEN";

/// Environment variable read for the base URL when none is configured.
pub const BASE_URL_ENV: &str = "ANTHROPIC_BASE_URL";

/// Default base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";

/// Value of the `anthropic-version` header.
pub const API_VERSION: &str = "2023-06-01";

/// Header carrying the comma-separated beta flags.
pub const BETA_HEADER: &str = "anthropic-beta";

/// Canonical provider options key, always consulted in addition to the
/// configured name.
pub const CANONICAL_OPTIONS_KEY: &str = "anthropic";

/// How requests authenticate.
#[derive(Clone)]
pub enum Credential {
    /// `x-api-key: <key>`.
    ApiKey(SecretString),
    /// `authorization: Bearer <token>`.
    AuthToken(SecretString),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("Credential::ApiKey(***)"),
            Self::AuthToken(_) => f.write_str("Credential::AuthToken(***)"),
        }
    }
}

/// Configuration shared by the models and services of one provider instance.
///
/// Built by [`crate::create_anthropic`]; exposed so that compatible endpoints
/// can reuse the Anthropic model types with their own settings.
pub struct AnthropicConfig {
    /// Provider name used as the prefix of every provider id (`anthropic`).
    pub name: String,
    /// Base URL without trailing slash (`https://api.anthropic.com/v1`).
    pub base_url: Url,
    /// Credential; loaded lazily from the environment when `None`.
    pub credential: Option<Credential>,
    /// Extra headers sent with every request.
    pub headers: Headers,
    /// Security policy for server-provided URLs (HTTPS and public networks by default).
    pub url_policy: UrlPolicy,
    /// HTTP transport.
    pub transport: SharedTransport,
    /// Generator for synthetic ids (sources).
    pub id_generator: Arc<dyn IdGenerator>,
    /// Whether function tools may carry `strict: true` (structured outputs
    /// beta); disable for compatible endpoints that reject the field.
    pub supports_strict_tools: bool,
    /// Whether `output_config.format` (native structured output) may be
    /// used; disable for compatible endpoints that lack it.
    pub supports_native_structured_output: bool,
}

impl fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnthropicConfig")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("credential", &self.credential)
            .field("headers", &self.headers)
            .field("supports_strict_tools", &self.supports_strict_tools)
            .field(
                "supports_native_structured_output",
                &self.supports_native_structured_output,
            )
            .finish_non_exhaustive()
    }
}

impl AnthropicConfig {
    /// Creates a configuration with the default transport and id generator.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Other`] when the default HTTP transport cannot
    /// be built.
    pub fn new(name: impl Into<String>, base_url: Url) -> Result<Self, ProviderError> {
        let transport = ferrin_provider_util::default_transport().map_err(ProviderError::other)?;
        Ok(Self::with_transport(name, base_url, transport))
    }

    /// Creates a configuration with an explicit transport.
    #[must_use]
    pub fn with_transport(
        name: impl Into<String>,
        base_url: Url,
        transport: SharedTransport,
    ) -> Self {
        Self {
            name: name.into(),
            base_url,
            credential: None,
            headers: Headers::new(),
            url_policy: UrlPolicy::default(),
            transport,
            id_generator: Arc::new(PrefixedIdGenerator::default()),
            supports_strict_tools: true,
            supports_native_structured_output: true,
        }
    }

    /// Provider id of an API family (`<name>.<family>`).
    #[must_use]
    pub fn provider_id(&self, family: &str) -> ProviderId {
        ProviderId::new(format!("{}.{family}", self.name))
    }

    /// Key under which provider options addressed to this instance are read
    /// in addition to [`CANONICAL_OPTIONS_KEY`]: the configured name.
    #[must_use]
    pub fn options_key(&self) -> &str {
        &self.name
    }

    /// Full URL of an API path (`/messages`).
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        join_path(&self.base_url, path)
    }

    /// Resolves the credential from the configuration or the environment
    /// (`ANTHROPIC_API_KEY`, then `ANTHROPIC_AUTH_TOKEN`).
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::LoadApiKey`] when nothing is configured.
    pub fn credential(&self) -> Result<Credential, ProviderError> {
        if let Some(credential) = &self.credential {
            return Ok(credential.clone());
        }
        if let Some(token) = ferrin_provider_util::settings::env_var(AUTH_TOKEN_ENV)
            && ferrin_provider_util::settings::env_var(API_KEY_ENV).is_none()
        {
            return Ok(Credential::AuthToken(SecretString::from(token)));
        }
        Ok(Credential::ApiKey(load_api_key(ApiKeyConfig {
            api_key: None,
            environment_variable: API_KEY_ENV,
            parameter_name: "api_key",
            description: "Anthropic",
        })?))
    }

    /// Request headers: credential, API version, configured headers, the
    /// per-call headers, the merged `anthropic-beta` header and the
    /// user-agent suffix.
    ///
    /// `betas` are joined with the beta flags already present in the
    /// configured and per-call headers.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::LoadApiKey`] when no credential is available
    /// and [`ProviderError::InvalidArgument`] when a value is not a valid
    /// header.
    pub fn headers(
        &self,
        call_headers: &Headers,
        betas: &BTreeSet<String>,
    ) -> Result<Headers, ProviderError> {
        let mut headers = Headers::new();
        match self.credential()? {
            Credential::ApiKey(key) => insert(&mut headers, "x-api-key", key.expose_secret())?,
            Credential::AuthToken(token) => insert(
                &mut headers,
                "authorization",
                &format!("Bearer {}", token.expose_secret()),
            )?,
        }
        insert(&mut headers, "anthropic-version", API_VERSION)?;
        headers.merge(&self.headers);
        headers.merge(call_headers);
        let mut all_betas = self.betas_from_headers(call_headers);
        all_betas.extend(betas.iter().cloned());
        if all_betas.is_empty() {
            headers.remove(BETA_HEADER);
        } else {
            let joined = all_betas.into_iter().collect::<Vec<_>>().join(",");
            insert(&mut headers, BETA_HEADER, &joined)?;
        }
        Ok(headers.with_user_agent_suffix([USER_AGENT]))
    }

    /// Beta flags found in the configured and per-call headers, lower-cased
    /// and trimmed.
    #[must_use]
    pub fn betas_from_headers(&self, call_headers: &Headers) -> BTreeSet<String> {
        let mut betas = BTreeSet::new();
        for source in [&self.headers, call_headers] {
            if let Some(value) = source.get_str(BETA_HEADER) {
                betas.extend(
                    value
                        .split(',')
                        .map(|beta| beta.trim().to_ascii_lowercase())
                        .filter(|beta| !beta.is_empty()),
                );
            }
        }
        betas
    }

    /// Generates a synthetic id.
    #[must_use]
    pub fn generate_id(&self) -> String {
        self.id_generator.generate()
    }
}

/// Normalizes a base URL: a bare origin (`https://api.anthropic.com`) gets
/// the `/v1` path appended, trailing slashes are removed.
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] for an empty or unparsable URL.
pub fn normalize_base_url(base_url: &str) -> Result<Url, InvalidArgumentError> {
    let mut url = ferrin_provider_util::base_url::parse_base_url(base_url)?;
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/v1");
    }
    Ok(url)
}

fn insert(headers: &mut Headers, name: &str, value: &str) -> Result<(), ProviderError> {
    headers.insert(name, value).map_err(|_| {
        ProviderError::InvalidArgument(InvalidArgumentError::new(
            name,
            format!("`{name}` is not a valid header value"),
        ))
    })
}

/// Shared handle to the configuration.
pub type SharedConfig = Arc<AnthropicConfig>;
