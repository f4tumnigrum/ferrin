//! Voyage reranking request and response conversion.
//!
//! Derived from Vercel AI SDK `packages/voyage/src/reranking/voyage-reranking-model.ts`
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); translated and modified.

use std::collections::HashSet;

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RerankingModel;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::reranking_model::RankedDocument;
use ferrin_spec::reranking_model::RerankDocuments;
use ferrin_spec::reranking_model::RerankOptions;
use ferrin_spec::reranking_model::RerankResult;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::options::parse_options;

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_documents: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation: Option<bool>,
}

#[derive(Deserialize)]
struct Response {
    data: Vec<RankedDocument>,
    #[serde(default)]
    model: Option<ModelId>,
}

/// Reranking model backed by Voyage's `POST /rerank` endpoint.
#[derive(Debug, Clone)]
pub struct VoyageRerankingModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl VoyageRerankingModel {
    /// Creates a model using shared provider configuration.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: ProviderId::new(format!("{}.reranking", config.name)),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared provider configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Converts options to a JSON request body and compatibility warnings.
    ///
    /// # Errors
    ///
    /// Returns an error for empty documents, zero `top_n`, unsupported
    /// document kinds, invalid provider options or serialization failure.
    pub fn prepare_request(
        &self,
        options: &RerankOptions,
    ) -> Result<(JsonValue, Vec<Warning>), ProviderError> {
        if options.documents.is_empty() {
            return Err(
                InvalidArgumentError::new("documents", "documents must not be empty").into(),
            );
        }
        if options.top_n == Some(0) {
            return Err(InvalidArgumentError::new("top_n", "top_n must be positive").into());
        }
        let provider_options = parse_options(&self.config.name, &options.provider_options)?;
        let mut warnings = Vec::new();
        let documents = match &options.documents {
            RerankDocuments::Text { values } => values.clone(),
            RerankDocuments::Object { values } => {
                warnings.push(Warning::compatibility(
                    "object documents",
                    Some("object documents are converted to strings".to_owned()),
                ));
                values
                    .iter()
                    .map(serde_json::to_string)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(ProviderError::other)?
            }
            _ => return Err(UnsupportedFunctionalityError::new("document kind").into()),
        };
        let request = Request {
            model: self.model_id.as_str(),
            query: &options.query,
            documents,
            top_k: options.top_n,
            return_documents: provider_options.return_documents,
            truncation: provider_options.truncation,
        };
        Ok((
            serde_json::to_value(request).map_err(ProviderError::other)?,
            warnings,
        ))
    }
}

impl RerankingModel for VoyageRerankingModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_rerank(&self, options: RerankOptions) -> Result<RerankResult, ProviderError> {
        let (body, warnings) = self.prepare_request(&options)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<Response>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("/rerank"),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation,
        )
        .await?;
        validate_ranking(&response.value.data, options.documents.len(), options.top_n)?;
        Ok(RerankResult {
            ranking: response.value.data,
            provider_metadata: None,
            warnings,
            response: ResponseMetadata {
                model_id: Some(
                    response
                        .value
                        .model
                        .unwrap_or_else(|| self.model_id.clone()),
                ),
                headers: Some(response.response_headers),
                body: response.raw,
                ..ResponseMetadata::default()
            },
        })
    }
}

fn validate_ranking(
    ranking: &[RankedDocument],
    document_count: usize,
    top_n: Option<usize>,
) -> Result<(), ProviderError> {
    let limit = top_n.unwrap_or(document_count).min(document_count);
    if ranking.len() > limit {
        return Err(InvalidResponseDataError::new(
            "ranking contains more results than requested",
            json!({"ranking_count": ranking.len(), "limit": limit}),
        )
        .into());
    }
    let mut seen = HashSet::with_capacity(ranking.len());
    let mut previous_score = f64::INFINITY;
    for entry in ranking {
        let message = if entry.index >= document_count {
            Some("ranking index is out of range")
        } else if !seen.insert(entry.index) {
            Some("ranking contains a duplicate index")
        } else if !entry.relevance_score.is_finite() {
            Some("ranking score must be finite")
        } else if entry.relevance_score > previous_score {
            Some("ranking scores must be in descending order")
        } else {
            None
        };
        if let Some(message) = message {
            return Err(InvalidResponseDataError::new(
                message,
                json!({"index": entry.index, "relevance_score": entry.relevance_score}),
            )
            .into());
        }
        previous_score = entry.relevance_score;
    }
    Ok(())
}
