//! Reranking model interface.

use std::future::Future;

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::json::JsonObject;
use crate::language_model::ResponseMetadata;
use crate::shared::Headers;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;

/// A model that orders documents by relevance to a query.
pub trait RerankingModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Ranks `options.documents` against `options.query`.
    fn do_rerank(
        &self,
        options: RerankOptions,
    ) -> impl Future<Output = Result<RerankResult, ProviderError>> + Send;
}

/// Documents to rank: all strings or all JSON objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum RerankDocuments {
    /// Plain text documents.
    Text {
        /// The documents.
        values: Vec<String>,
    },
    /// Structured documents.
    Object {
        /// The documents.
        values: Vec<JsonObject>,
    },
}

impl RerankDocuments {
    /// Number of documents.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Text { values } => values.len(),
            Self::Object { values } => values.len(),
        }
    }

    /// Returns `true` when there are no documents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Options for a rerank call.
#[derive(Debug, Clone)]
pub struct RerankOptions {
    /// The query.
    pub query: String,
    /// Documents to rank.
    pub documents: RerankDocuments,
    /// Return only the top `n` results.
    pub top_n: Option<usize>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl RerankOptions {
    /// Creates options for `query` over `documents`.
    #[must_use]
    pub fn new(query: impl Into<String>, documents: RerankDocuments) -> Self {
        Self {
            query: query.into(),
            documents,
            top_n: None,
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// One entry of a ranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedDocument {
    /// Index of the document in the input list.
    pub index: usize,
    /// Relevance score (higher is more relevant).
    pub relevance_score: f64,
}

/// Result of a rerank call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankResult {
    /// Ranking, most relevant first.
    pub ranking: Vec<RankedDocument>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Response metadata.
    #[serde(default)]
    pub response: ResponseMetadata,
}
