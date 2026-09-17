//! Shared configuration of every OpenAI model and service.

use std::fmt;
use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::PrefixedIdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::join_path;
use ferrin_provider_util::settings::ApiKeyConfig;
use ferrin_provider_util::settings::load_api_key;
use ferrin_spec::Headers;
use ferrin_spec::ProviderId;
use ferrin_spec::error::ProviderError;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use url::Url;

/// User-agent suffix appended to every request.
pub const USER_AGENT: &str = concat!("ferrin-openai/", env!("CARGO_PKG_VERSION"));

/// Environment variable read for the API key when none is configured.
pub const API_KEY_ENV: &str = "OPENAI_API_KEY";

/// Environment variable read for the base URL when none is configured.
pub const BASE_URL_ENV: &str = "OPENAI_BASE_URL";

/// Default base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Prefixes that identify uploaded file ids inside string file data.
pub const FILE_ID_PREFIXES: &[&str] = &["file-"];

/// Who supplies authentication for an OpenAI-compatible request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Authentication {
    /// Load and send an OpenAI bearer API key.
    #[default]
    OpenAi,
    /// The explicitly configured transport supplies authentication.
    External,
}

/// Configuration shared by the models and services of one provider instance.
///
/// Built by [`crate::create_openai`]; exposed so that compatible endpoints can
/// reuse the OpenAI model types with their own settings.
pub struct OpenAiConfig {
    /// Provider name used as the prefix of every provider id (`openai`).
    pub name: String,
    /// Base URL without trailing slash (`https://api.openai.com/v1`).
    pub base_url: Url,
    /// API key; loaded lazily from [`API_KEY_ENV`] when `None`.
    pub api_key: Option<SecretString>,
    /// Authentication source; defaults to OpenAI API-key authentication.
    pub authentication: Authentication,
    /// `OpenAI-Organization` header value.
    pub organization: Option<String>,
    /// `OpenAI-Project` header value.
    pub project: Option<String>,
    /// Extra headers sent with every request.
    pub headers: Headers,
    /// HTTP transport.
    pub transport: SharedTransport,
    /// Generator for synthetic ids (sources, approval tool calls).
    pub id_generator: Arc<dyn IdGenerator>,
    /// Key under which provider options and metadata are read and written
    /// (`openai`).
    pub provider_options_key: String,
    /// Whether Responses input messages carry an explicit `type: "message"`.
    pub explicit_message_item_type: bool,
    /// Whether the endpoint accepts the `web_search_call.action.sources`
    /// include value.
    pub supports_web_search_sources_include: bool,
    /// Prefixes identifying file ids in string file data.
    pub file_id_prefixes: Vec<String>,
}

impl fmt::Debug for OpenAiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiConfig")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("authentication", &self.authentication)
            .field("organization", &self.organization)
            .field("project", &self.project)
            .field("headers", &self.headers)
            .field("provider_options_key", &self.provider_options_key)
            .field(
                "explicit_message_item_type",
                &self.explicit_message_item_type,
            )
            .field(
                "supports_web_search_sources_include",
                &self.supports_web_search_sources_include,
            )
            .field("file_id_prefixes", &self.file_id_prefixes)
            .finish_non_exhaustive()
    }
}

impl OpenAiConfig {
    /// Creates a configuration with the default transport, id generator and
    /// OpenAI defaults for the remaining fields.
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
            api_key: None,
            authentication: Authentication::OpenAi,
            organization: None,
            project: None,
            headers: Headers::new(),
            transport,
            id_generator: Arc::new(PrefixedIdGenerator::default()),
            provider_options_key: "openai".to_owned(),
            explicit_message_item_type: false,
            supports_web_search_sources_include: true,
            file_id_prefixes: FILE_ID_PREFIXES.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    /// Creates a configuration whose transport supplies authentication.
    ///
    /// No OpenAI API key is loaded or sent. This is intended for provider
    /// adapters with a dedicated credential-aware transport.
    #[must_use]
    pub fn with_external_authentication(
        name: impl Into<String>,
        base_url: Url,
        transport: SharedTransport,
    ) -> Self {
        let mut config = Self::with_transport(name, base_url, transport);
        config.authentication = Authentication::External;
        config
    }

    /// Provider id of an API family (`<name>.<family>`).
    #[must_use]
    pub fn provider_id(&self, family: &str) -> ProviderId {
        ProviderId::new(format!("{}.{family}", self.name))
    }

    /// Full URL of an API path (`/responses`).
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        join_path(&self.base_url, path)
    }

    /// Resolves the API key from the configuration or the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::LoadApiKey`] when neither is set.
    pub fn api_key(&self) -> Result<SecretString, ProviderError> {
        Ok(load_api_key(ApiKeyConfig {
            api_key: self.api_key.clone(),
            environment_variable: API_KEY_ENV,
            parameter_name: "api_key",
            description: "OpenAI",
        })?)
    }

    /// Request headers: authorization, organization, project, configured
    /// headers, then the per-call headers and the user-agent suffix.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::LoadApiKey`] when no API key is available
    /// and [`ProviderError::InvalidArgument`] when a configured value is not
    /// a valid header.
    pub fn headers(&self, call_headers: &Headers) -> Result<Headers, ProviderError> {
        let mut headers = Headers::new();
        if self.authentication == Authentication::OpenAi {
            let key = self.api_key()?;
            insert(
                &mut headers,
                "authorization",
                &format!("Bearer {}", key.expose_secret()),
            )?;
        }
        if let Some(organization) = &self.organization {
            insert(&mut headers, "openai-organization", organization)?;
        }
        if let Some(project) = &self.project {
            insert(&mut headers, "openai-project", project)?;
        }
        headers.merge(&self.headers);
        headers.merge(call_headers);
        Ok(headers.with_user_agent_suffix([USER_AGENT]))
    }

    /// Generates a synthetic id.
    #[must_use]
    pub fn generate_id(&self) -> String {
        self.id_generator.generate()
    }

    /// Builds the WebSocket URL of `path` under the base URL (`wss` for
    /// `https`, `ws` for `http`) with `query` appended.
    #[must_use]
    pub fn websocket_url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.url(path);
        let scheme = match url.scheme() {
            "https" => Some("wss"),
            "http" => Some("ws"),
            _ => None,
        };
        if let Some(scheme) = scheme {
            let _ = url.set_scheme(scheme);
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        url
    }
}

fn insert(headers: &mut Headers, name: &str, value: &str) -> Result<(), ProviderError> {
    headers.insert(name, value).map_err(|_| {
        ProviderError::InvalidArgument(ferrin_spec::error::InvalidArgumentError::new(
            name,
            format!("`{name}` is not a valid header value"),
        ))
    })
}

/// Shared handle to the configuration.
pub type SharedConfig = Arc<OpenAiConfig>;
