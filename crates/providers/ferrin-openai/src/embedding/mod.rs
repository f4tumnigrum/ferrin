//! Embedding model (`<name>.embedding`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::embedding_model::EmbeddingModel;
use ferrin_spec::embedding_model::EmbeddingUsage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TooManyEmbeddingValuesForCallError;
use serde::Deserialize;
use serde::Serialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;

/// Maximum number of values per call.
pub const MAX_EMBEDDINGS_PER_CALL: usize = 2048;
/// Maximum total input bytes per call.
pub const MAX_INPUT_BYTES_PER_CALL: usize = 300_000;

/// Provider options of the embedding model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingProviderOptions {
    /// Number of output dimensions (text-embedding-3 and later).
    #[serde(default)]
    pub dimensions: Option<u32>,
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
    encoding_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    #[serde(default)]
    data: Vec<EmbeddingData>,
    #[serde(default)]
    usage: Option<EmbeddingResponseUsage>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponseUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
}

/// Embedding model backed by `POST /embeddings`.
#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiEmbeddingModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("embedding"),
            config,
            model_id: model_id.into(),
        }
    }
}

impl EmbeddingModel for OpenAiEmbeddingModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        Some(MAX_EMBEDDINGS_PER_CALL)
    }

    fn max_input_bytes_per_call(&self) -> Option<usize> {
        Some(MAX_INPUT_BYTES_PER_CALL)
    }

    fn supports_parallel_calls(&self) -> bool {
        true
    }

    async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
        if options.values.len() > MAX_EMBEDDINGS_PER_CALL {
            return Err(TooManyEmbeddingValuesForCallError {
                provider: self.provider.clone(),
                model_id: self.model_id.clone(),
                max_embeddings_per_call: MAX_EMBEDDINGS_PER_CALL,
                value_count: options.values.len(),
            }
            .into());
        }
        let openai = parse_provider_options::<EmbeddingProviderOptions>(
            &self.config.provider_options_key,
            &options.provider_options,
        )?
        .unwrap_or_default();
        let body = EmbeddingRequest {
            model: self.model_id.as_str(),
            input: &options.values,
            encoding_format: "float",
            dimensions: openai.dimensions,
            user: openai.user.as_deref(),
        };
        let handlers = ResponseHandlers::new(
            json_response_handler::<EmbeddingResponse>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("/embeddings"),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation,
        )
        .await?;
        let value = response.value;
        Ok(EmbedResult {
            embeddings: value.data.into_iter().map(|d| d.embedding).collect(),
            usage: value
                .usage
                .and_then(|u| u.prompt_tokens)
                .map(|tokens| EmbeddingUsage { tokens }),
            provider_metadata: None,
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            warnings: Vec::new(),
        })
    }
}

/// Serializes a JSON object into provider metadata under the configured key.
pub(crate) fn provider_metadata(key: &str, value: JsonObject) -> ferrin_spec::ProviderMetadata {
    let mut map = ferrin_spec::ProviderMetadata::new();
    map.insert(key.to_owned(), value);
    map
}

/// Removes `null` entries from a JSON object.
pub(crate) fn compact(mut object: JsonObject) -> JsonObject {
    object.retain(|_, value| !matches!(value, JsonValue::Null));
    object
}
