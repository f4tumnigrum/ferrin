//! Embedding model (`<name>.embedding`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::embedding_model::EmbeddingModel;
use ferrin_spec::embedding_model::EmbeddingUsage;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TooManyEmbeddingValuesForCallError;
use serde::Deserialize;
use serde::Serialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::options_key::DEPRECATED_SHARED_OPTIONS_KEY;
use crate::options_key::SHARED_OPTIONS_KEY;
use crate::options_key::merged_options;
use crate::options_key::option_keys;
use crate::options_key::warn_if_deprecated_key;

/// Provider options of the embedding model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingOptions {
    /// Number of output dimensions.
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
    #[serde(default, rename = "providerMetadata")]
    provider_metadata: Option<ProviderMetadata>,
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
pub struct OpenAiCompatibleEmbeddingModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiCompatibleEmbeddingModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("embedding"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }
}

impl EmbeddingModel for OpenAiCompatibleEmbeddingModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        Some(self.config.max_embeddings_per_call)
    }

    fn supports_parallel_calls(&self) -> bool {
        self.config.supports_parallel_calls
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
        let name = &self.config.name;
        let mut warnings = Vec::new();
        if options
            .provider_options
            .contains_key(DEPRECATED_SHARED_OPTIONS_KEY)
        {
            warnings.push(Warning::deprecated(
                format!("providerOptions key '{DEPRECATED_SHARED_OPTIONS_KEY}'"),
                format!("Use '{SHARED_OPTIONS_KEY}' instead."),
            ));
        }
        warn_if_deprecated_key(name, &options.provider_options, &mut warnings);
        let compatible: EmbeddingOptions =
            match merged_options(&option_keys(name), &options.provider_options) {
                Some(object) => {
                    serde_json::from_value(JsonValue::Object(object)).map_err(|error| {
                        InvalidArgumentError::new(
                            "provider_options",
                            format!("invalid {name} provider options: {error}"),
                        )
                    })?
                }
                None => EmbeddingOptions::default(),
            };
        if options.values.len() > self.config.max_embeddings_per_call {
            return Err(TooManyEmbeddingValuesForCallError {
                provider: self.provider.clone(),
                model_id: self.model_id.clone(),
                max_embeddings_per_call: self.config.max_embeddings_per_call,
                value_count: options.values.len(),
            }
            .into());
        }
        let body = EmbeddingRequest {
            model: self.model_id.as_str(),
            input: &options.values,
            encoding_format: "float",
            dimensions: compatible.dimensions,
            user: compatible.user.as_deref(),
        };
        let handlers = ResponseHandlers::new(
            json_response_handler::<EmbeddingResponse>(),
            failed_response_handler(self.config.error_structure.clone()),
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
            provider_metadata: value.provider_metadata,
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            warnings,
        })
    }
}
