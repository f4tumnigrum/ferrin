//! Shared configuration of every model of one OpenAI-compatible provider
//! instance.

use std::fmt;
use std::sync::Arc;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::PrefixedIdGenerator;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::base_url::join_path;
use ferrin_provider_util::settings::env_var;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::ProviderId;
use ferrin_spec::Usage;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::SupportedUrls;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use url::Url;

use crate::chat::api_types::ChatUsage;
use crate::error::DefaultErrorStructure;
use crate::error::SharedErrorStructure;
use crate::metadata::SharedMetadataExtractor;

/// User-agent suffix appended to every request.
pub const USER_AGENT: &str = concat!("ferrin-openai-compatible/", env!("CARGO_PKG_VERSION"));

/// Default maximum number of values per embedding call.
pub const DEFAULT_MAX_EMBEDDINGS_PER_CALL: usize = 2048;

/// Hook rewriting a request body before it is sent (proxies that expect a
/// different shape).
pub type TransformRequestBody = Arc<dyn Fn(JsonObject) -> JsonObject + Send + Sync>;

/// Hook converting the chat usage object for endpoints with different token
/// accounting.
pub type ConvertUsage = Arc<dyn Fn(&ChatUsage) -> Usage + Send + Sync>;

/// Configuration shared by the models of one provider instance.
pub struct OpenAiCompatibleConfig {
    /// Provider name used as the prefix of every provider id and as the
    /// provider options key.
    pub name: String,
    /// Base URL without trailing slash.
    pub base_url: Url,
    /// API key sent as `authorization: Bearer <key>`.
    pub api_key: Option<SecretString>,
    /// Environment variable read for the API key when `api_key` is `None`.
    pub api_key_env: Option<String>,
    /// Extra headers sent with every request.
    pub headers: Headers,
    /// Query parameters appended to every request URL.
    pub query_params: Vec<(String, String)>,
    /// HTTP transport.
    pub transport: SharedTransport,
    /// Generator for synthetic ids (tool calls without an id).
    pub id_generator: Arc<dyn IdGenerator>,
    /// Whether streaming requests ask for usage (`stream_options.include_usage`).
    pub include_usage: bool,
    /// Whether the chat model supports `response_format.json_schema`.
    pub supports_structured_outputs: bool,
    /// URLs the chat model accepts as file parts.
    pub supported_urls: SupportedUrls,
    /// Error body structure.
    pub error_structure: SharedErrorStructure,
    /// Metadata extractor of the chat model.
    pub metadata_extractor: Option<SharedMetadataExtractor>,
    /// Request body transformer of the chat model.
    pub transform_request_body: Option<TransformRequestBody>,
    /// Usage converter of the chat model.
    pub convert_usage: Option<ConvertUsage>,
    /// Maximum values per embedding call.
    pub max_embeddings_per_call: usize,
    /// Whether embedding calls may run in parallel.
    pub supports_parallel_calls: bool,
}

impl fmt::Debug for OpenAiCompatibleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatibleConfig")
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
            .field("error_structure", &self.error_structure)
            .field("metadata_extractor", &self.metadata_extractor)
            .field("max_embeddings_per_call", &self.max_embeddings_per_call)
            .field("supports_parallel_calls", &self.supports_parallel_calls)
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatibleConfig {
    /// Creates a configuration with the default transport and defaults for
    /// every optional field.
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
            api_key_env: None,
            headers: Headers::new(),
            query_params: Vec::new(),
            transport,
            id_generator: Arc::new(PrefixedIdGenerator::default()),
            include_usage: false,
            supports_structured_outputs: false,
            supported_urls: SupportedUrls::none(),
            error_structure: Arc::new(DefaultErrorStructure),
            metadata_extractor: None,
            transform_request_body: None,
            convert_usage: None,
            max_embeddings_per_call: DEFAULT_MAX_EMBEDDINGS_PER_CALL,
            supports_parallel_calls: true,
        }
    }

    /// Provider id of a model family (`<name>.<family>`).
    #[must_use]
    pub fn provider_id(&self, family: &str) -> ProviderId {
        ProviderId::new(format!("{}.{family}", self.name))
    }

    /// Full URL of an API path (`/chat/completions`) with the configured
    /// query parameters.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        let mut url = join_path(&self.base_url, path);
        if !self.query_params.is_empty() {
            url.query_pairs_mut().extend_pairs(
                self.query_params
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            );
        }
        url
    }

    /// The API key: configured, else read from `api_key_env`; `None` when
    /// the endpoint needs no credential.
    #[must_use]
    pub fn api_key(&self) -> Option<SecretString> {
        if let Some(key) = &self.api_key {
            return Some(key.clone());
        }
        let variable = self.api_key_env.as_deref()?;
        env_var(variable).map(SecretString::from)
    }

    /// Request headers: bearer authorization when a key is available, the
    /// configured headers, the per-call headers and the user-agent suffix.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidArgument`] when the key is not a valid
    /// header value.
    pub fn headers(&self, call_headers: &Headers) -> Result<Headers, ProviderError> {
        let mut headers = Headers::new();
        if let Some(key) = self.api_key() {
            headers
                .insert("authorization", &format!("Bearer {}", key.expose_secret()))
                .map_err(|_| {
                    ProviderError::InvalidArgument(InvalidArgumentError::new(
                        "api_key",
                        "api_key is not a valid header value",
                    ))
                })?;
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

    /// Applies the request body transformer.
    #[must_use]
    pub fn transform_request_body(&self, body: JsonObject) -> JsonObject {
        match &self.transform_request_body {
            Some(transform) => transform(body),
            None => body,
        }
    }
}

/// Shared handle to the configuration.
pub type SharedConfig = Arc<OpenAiCompatibleConfig>;
