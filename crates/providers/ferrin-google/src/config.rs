//! Shared configuration of every Google model and service.

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
pub const USER_AGENT: &str = concat!("ferrin-google/", env!("CARGO_PKG_VERSION"));

/// Environment variable read for the API key when none is configured.
pub const API_KEY_ENV: &str = "GOOGLE_GENERATIVE_AI_API_KEY";

/// Default base URL (Gemini API, `v1beta`).
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Header carrying the API key.
pub const API_KEY_HEADER: &str = "x-goog-api-key";

/// Canonical provider options and metadata key, always consulted in addition
/// to the configured name.
pub const CANONICAL_OPTIONS_KEY: &str = "google";

/// Default provider name.
pub const DEFAULT_NAME: &str = "google";

/// Path of the resumable upload endpoint (relative to the origin).
pub const UPLOAD_PATH: &str = "/upload/v1beta/files";

/// Path prefix of the media download endpoint (relative to the origin).
pub const DOWNLOAD_PATH_PREFIX: &str = "/download/v1beta/";

/// Path of the ephemeral auth token endpoint (relative to the origin).
pub const AUTH_TOKENS_PATH: &str = "/v1alpha/auth_tokens";

/// Configuration shared by the models and services of one provider instance.
///
/// Built by [`crate::create_google`]; exposed so that compatible endpoints
/// can reuse the model types with their own settings.
pub struct GoogleConfig {
    /// Provider name used as the prefix of every provider id (`google`).
    pub name: String,
    /// Base URL without trailing slash
    /// (`https://generativelanguage.googleapis.com/v1beta`).
    pub base_url: Url,
    /// API key; loaded lazily from `GOOGLE_GENERATIVE_AI_API_KEY` when `None`.
    pub api_key: Option<SecretString>,
    /// Extra headers sent with every request.
    pub headers: Headers,
    /// Security policy for server-provided URLs (HTTPS and public networks by default).
    pub url_policy: UrlPolicy,
    /// HTTP transport.
    pub transport: SharedTransport,
    /// Generator for synthetic ids (tool calls without an id, sources).
    pub id_generator: Arc<dyn IdGenerator>,
}

impl fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleConfig")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl GoogleConfig {
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
            api_key: None,
            headers: Headers::new(),
            url_policy: UrlPolicy::default(),
            transport,
            id_generator: Arc::new(PrefixedIdGenerator::default()),
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

    /// Full URL of an API path below the base URL (`/models/x:generateContent`).
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        join_path(&self.base_url, path)
    }

    /// Resource path of a model: ids containing `/` are used as-is, others
    /// are prefixed with `models/`.
    #[must_use]
    pub fn model_path(model_id: &str) -> String {
        if model_id.contains('/') {
            model_id.to_owned()
        } else {
            format!("models/{model_id}")
        }
    }

    /// URL of a model action (`{base}/{model path}:{action}`).
    #[must_use]
    pub fn model_url(&self, model_id: &str, action: &str) -> Url {
        self.url(&format!("{}:{action}", Self::model_path(model_id)))
    }

    /// URL of an absolute path on the base URL's origin (used by the upload,
    /// download and auth token endpoints, which are not nested under the API
    /// version).
    #[must_use]
    pub fn origin_url(&self, path: &str) -> Url {
        let mut url = self.base_url.clone();
        url.set_path(path);
        url.set_query(None);
        url.set_fragment(None);
        url
    }

    /// WebSocket URL of a Live API service: the trailing `v1beta`/`v1alpha`
    /// segment of the base URL is removed, the scheme becomes `wss`/`ws` and
    /// `/ws/<service path>` is appended.
    #[must_use]
    pub fn websocket_url(&self, service_path: &str) -> Url {
        let mut url = self.base_url.clone();
        let mut segments: Vec<&str> = url
            .path()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if matches!(segments.last(), Some(&"v1beta" | &"v1alpha")) {
            segments.pop();
        }
        let mut path = segments.join("/");
        if !path.is_empty() {
            path.insert(0, '/');
        }
        path.push_str("/ws/");
        path.push_str(service_path);
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        let scheme = if url.scheme() == "http" { "ws" } else { "wss" };
        // Changing `http(s)` to `ws(s)` is always accepted by the URL parser.
        let _ = url.set_scheme(scheme);
        url
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
            description: "Google Generative AI",
        })?)
    }

    /// Request headers: API key, configured headers, per-call headers and
    /// the user-agent suffix.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::LoadApiKey`] when no API key is available and
    /// [`ProviderError::InvalidArgument`] when the key is not a valid header
    /// value.
    pub fn headers(&self, call_headers: &Headers) -> Result<Headers, ProviderError> {
        let mut headers = Headers::new();
        let key = self.api_key()?;
        headers
            .insert(API_KEY_HEADER, key.expose_secret())
            .map_err(|_| {
                ProviderError::InvalidArgument(InvalidArgumentError::new(
                    "api_key",
                    "api_key is not a valid header value",
                ))
            })?;
        headers.merge(&self.headers);
        headers.merge(call_headers);
        Ok(headers.with_user_agent_suffix([USER_AGENT]))
    }

    /// Headers of requests that must not carry the API key (resumable upload
    /// sessions, ephemeral token creation): configured headers, `call_headers`
    /// and the user-agent suffix.
    #[must_use]
    pub fn unauthenticated_headers(&self, call_headers: &Headers) -> Headers {
        let mut headers = self.headers.clone();
        headers.merge(call_headers);
        headers.with_user_agent_suffix([USER_AGENT])
    }

    /// Generates a synthetic id.
    #[must_use]
    pub fn generate_id(&self) -> String {
        self.id_generator.generate()
    }
}

/// Shared handle to the configuration.
pub type SharedConfig = Arc<GoogleConfig>;
