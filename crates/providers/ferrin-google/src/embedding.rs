//! Gemini embedding model (`embedContent` / `batchEmbedContents`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::embedding_model::Embedding;
use ferrin_spec::embedding_model::EmbeddingModel;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TooManyEmbeddingValuesForCallError;
use serde::Deserialize;
use serde_json::json;

use crate::config::GoogleConfig;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::options::parse_merged;

/// Maximum values per call.
pub const MAX_EMBEDDINGS_PER_CALL: usize = 100;

/// Embedding options (`provider_options["google"]`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoogleEmbeddingOptions {
    /// Output dimensionality.
    #[serde(default)]
    pub output_dimensionality: Option<u32>,
    /// Task type (`SEMANTIC_SIMILARITY`, `RETRIEVAL_DOCUMENT`, ...).
    #[serde(default)]
    pub task_type: Option<String>,
    /// Extra multimodal parts per value (`[{text} | {inlineData} | {fileData}]`
    /// or `null`); must have one entry per value.
    #[serde(default)]
    pub content: Option<Vec<Option<Vec<JsonValue>>>>,
}

impl GoogleEmbeddingOptions {
    fn merge(mut self, other: Self) -> Self {
        if other.output_dimensionality.is_some() {
            self.output_dimensionality = other.output_dimensionality;
        }
        if other.task_type.is_some() {
            self.task_type = other.task_type;
        }
        if other.content.is_some() {
            self.content = other.content;
        }
        self
    }
}

#[derive(Debug, Deserialize)]
struct SingleEmbeddingResponse {
    embedding: EmbeddingValues,
}

#[derive(Debug, Deserialize)]
struct BatchEmbeddingResponse {
    #[serde(default)]
    embeddings: Vec<EmbeddingValues>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingValues {
    #[serde(default)]
    values: Embedding,
}

/// Embedding model backed by `embedContent`.
#[derive(Debug, Clone)]
pub struct GoogleEmbeddingModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

/// A prepared embedding request.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEmbeddingRequest {
    /// Action (`embedContent` or `batchEmbedContents`).
    pub action: &'static str,
    /// Request body.
    pub body: JsonValue,
}

fn parts(value: &str, extra: Option<&Vec<JsonValue>>) -> Vec<JsonValue> {
    let mut parts = Vec::new();
    match extra {
        Some(extra) => {
            if !value.is_empty() {
                parts.push(json!({"text": value}));
            }
            parts.extend(extra.iter().cloned());
        }
        None => parts.push(json!({"text": value})),
    }
    parts
}

impl GoogleEmbeddingModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: ProviderId::new(config.name.clone()),
            config,
            model_id: model_id.into(),
        }
    }

    /// Builds the request for `options`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::TooManyEmbeddingValues`] above
    /// [`MAX_EMBEDDINGS_PER_CALL`] and [`ProviderError::InvalidArgument`] for
    /// invalid options or a `content` list whose length differs from the
    /// values.
    pub fn prepare_request(
        &self,
        options: &EmbedOptions,
    ) -> Result<PreparedEmbeddingRequest, ProviderError> {
        if options.values.len() > MAX_EMBEDDINGS_PER_CALL {
            return Err(TooManyEmbeddingValuesForCallError {
                provider: self.provider.clone(),
                model_id: self.model_id.clone(),
                max_embeddings_per_call: MAX_EMBEDDINGS_PER_CALL,
                value_count: options.values.len(),
            }
            .into());
        }
        let google = parse_merged::<GoogleEmbeddingOptions>(
            &self.config,
            &options.provider_options,
            GoogleEmbeddingOptions::merge,
        )?;
        if let Some(content) = &google.content
            && content.len() != options.values.len()
        {
            return Err(InvalidArgumentError::new(
                "content",
                format!(
                    "the number of multimodal content entries ({}) must match the number of values ({})",
                    content.len(),
                    options.values.len()
                ),
            )
            .into());
        }
        let model = GoogleConfig::model_path(self.model_id.as_str());
        let extra = |index: usize| {
            google
                .content
                .as_ref()
                .and_then(|content| content.get(index))
                .and_then(Option::as_ref)
        };
        let common = |request: &mut JsonObject| {
            if let Some(dimensionality) = google.output_dimensionality {
                request.insert(
                    "outputDimensionality".to_owned(),
                    JsonValue::from(dimensionality),
                );
            }
            if let Some(task_type) = &google.task_type {
                request.insert("taskType".to_owned(), JsonValue::from(task_type.as_str()));
            }
        };
        if let [value] = options.values.as_slice() {
            let mut request = JsonObject::new();
            request.insert("model".to_owned(), JsonValue::from(model.as_str()));
            request.insert(
                "content".to_owned(),
                json!({"parts": parts(value, extra(0))}),
            );
            common(&mut request);
            return Ok(PreparedEmbeddingRequest {
                action: "embedContent",
                body: JsonValue::Object(request),
            });
        }
        let requests: Vec<JsonValue> = options
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let mut request = JsonObject::new();
                request.insert("model".to_owned(), JsonValue::from(model.as_str()));
                request.insert(
                    "content".to_owned(),
                    json!({"role": "user", "parts": parts(value, extra(index))}),
                );
                common(&mut request);
                JsonValue::Object(request)
            })
            .collect();
        Ok(PreparedEmbeddingRequest {
            action: "batchEmbedContents",
            body: json!({"requests": requests}),
        })
    }
}

impl EmbeddingModel for GoogleEmbeddingModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        Some(MAX_EMBEDDINGS_PER_CALL)
    }

    fn supports_parallel_calls(&self) -> bool {
        true
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let url = self
            .config
            .model_url(self.model_id.as_str(), prepared.action);
        let headers = self.config.headers(&options.headers)?;
        let (embeddings, response_headers, raw) = if prepared.action == "embedContent" {
            let handlers = ResponseHandlers::new(
                json_response_handler::<SingleEmbeddingResponse>(),
                failed_response_handler(),
            );
            let response = post_json(
                self.config.transport.as_ref(),
                url,
                headers,
                &prepared.body,
                &handlers,
                options.cancellation.clone(),
            )
            .await?;
            (
                vec![response.value.embedding.values],
                response.response_headers,
                response.raw,
            )
        } else {
            let handlers = ResponseHandlers::new(
                json_response_handler::<BatchEmbeddingResponse>(),
                failed_response_handler(),
            );
            let response = post_json(
                self.config.transport.as_ref(),
                url,
                headers,
                &prepared.body,
                &handlers,
                options.cancellation.clone(),
            )
            .await?;
            (
                response
                    .value
                    .embeddings
                    .into_iter()
                    .map(|embedding| embedding.values)
                    .collect(),
                response.response_headers,
                response.raw,
            )
        };
        Ok(EmbedResult {
            embeddings,
            usage: None,
            provider_metadata: None,
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response_headers),
                body: raw,
            },
            warnings: Vec::new(),
        })
    }
}
