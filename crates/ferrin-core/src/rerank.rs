//! Reranking: [`rerank`] orders documents by relevance to a query.
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §5.
//!
//! Lifecycle behavior is derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated to Rust and modified; see NOTICE.

use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonObject;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RerankingModelRef;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::reranking_model::RerankDocuments;
use ferrin_spec::reranking_model::RerankOptions;
use serde_json::json;
use tracing::Instrument;

use crate::error::Error;
use crate::hooks::Hooks;
use crate::ids::default_id_generator;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::modality_hooks::ModalityHooks;
pub use crate::modality_hooks::RerankCallEndEvent;
pub use crate::modality_hooks::RerankCallStartEvent;
use crate::modality_hooks::impl_modality_hooks;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::retry;
use crate::telemetry::ErrorEvent;
use crate::telemetry::ErrorPhase;
use crate::telemetry::ModelIdentity;
use crate::telemetry::RerankEndEvent;
use crate::telemetry::RerankStartEvent;
use crate::telemetry::dispatcher::TelemetryDispatcher;
use crate::telemetry::spans;

/// A document to rerank: text or a JSON object.
#[derive(Debug, Clone, PartialEq)]
pub enum RerankDocument {
    /// Plain text.
    Text(String),
    /// A structured document.
    Object(JsonObject),
}

impl From<String> for RerankDocument {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for RerankDocument {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<JsonObject> for RerankDocument {
    fn from(object: JsonObject) -> Self {
        Self::Object(object)
    }
}

/// One ranked document.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked<D> {
    /// Index of the document in the input list.
    pub original_index: usize,
    /// Relevance score reported by the model.
    pub score: f64,
    /// The document.
    pub document: D,
}

/// Result of [`rerank`].
#[derive(Debug, Clone, PartialEq)]
pub struct RerankResult<D> {
    /// The complete original document list, in input order.
    pub original_documents: Vec<D>,
    /// Documents in the order returned by the model (most relevant first).
    pub ranking: Vec<Ranked<D>>,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Response metadata.
    pub response: ResponseMetadata,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
}

impl<D> RerankResult<D> {
    /// The documents in ranked order.
    pub fn reranked_documents(&self) -> impl Iterator<Item = &D> + '_ {
        self.ranking.iter().map(|ranked| &ranked.document)
    }
}

/// Reranks `documents` by relevance to `query`. Documents must be all
/// text or all objects.
#[must_use]
pub fn rerank<D>(
    model: impl Into<RerankingModelRef>,
    query: impl Into<String>,
    documents: Vec<D>,
) -> Rerank<D>
where
    D: Into<RerankDocument> + Clone + Send + 'static,
{
    Rerank {
        model: model.into(),
        query: query.into(),
        documents,
        top_n: None,
        base: ModalityOptions::default(),
        hooks: ModalityHooks::default(),
    }
}

/// Builder returned by [`rerank`]; `.await` runs the call.
#[derive(Debug)]
pub struct Rerank<D> {
    model: RerankingModelRef,
    query: String,
    documents: Vec<D>,
    top_n: Option<usize>,
    base: ModalityOptions,
    hooks: ModalityHooks<RerankCallStartEvent, RerankCallEndEvent>,
}

impl<D> Rerank<D> {
    /// Returns only the `top_n` most relevant documents.
    #[must_use]
    pub fn top_n(mut self, top_n: usize) -> Self {
        self.top_n = Some(top_n);
        self
    }
}

impl_modality_builder!(Rerank<D>);
impl_modality_hooks!(Rerank<D>, RerankCallStartEvent, RerankCallEndEvent);

impl<D> IntoFuture for Rerank<D>
where
    D: Into<RerankDocument> + Clone + Send + 'static,
{
    type Output = Result<RerankResult<D>, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(self))
    }
}

/// Converts the documents to the provider representation.
fn to_model_documents<D>(documents: &[D]) -> Result<RerankDocuments, Error>
where
    D: Into<RerankDocument> + Clone,
{
    let mut texts: Vec<String> = Vec::new();
    let mut objects: Vec<JsonObject> = Vec::new();
    for document in documents {
        match document.clone().into() {
            RerankDocument::Text(text) => texts.push(text),
            RerankDocument::Object(object) => objects.push(object),
        }
    }
    match (texts.is_empty(), objects.is_empty()) {
        (false, true) => Ok(RerankDocuments::Text { values: texts }),
        (true, false) => Ok(RerankDocuments::Object { values: objects }),
        _ => Err(Error::invalid_argument(
            "documents",
            "documents must be all text or all objects",
        )),
    }
}

async fn run<D>(builder: Rerank<D>) -> Result<RerankResult<D>, Error>
where
    D: Into<RerankDocument> + Clone + Send + 'static,
{
    let model = resolve_model(&builder.model, ProviderRegistry::reranking_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("rerank", &identity);
    let base = builder.base.clone();
    let telemetry = TelemetryDispatcher::new(base.telemetry.clone());
    let call_id = default_id_generator().generate();
    let Rerank {
        query,
        documents,
        top_n,
        hooks,
        ..
    } = builder;
    let event_documents: Vec<RerankDocument> = documents.iter().cloned().map(Into::into).collect();
    let start = Arc::new(RerankCallStartEvent {
        runtime_context: Some(hooks.runtime_context.clone()),
        call_id: call_id.clone(),
        operation_id: "ai.rerank",
        model: identity.clone(),
        documents: Some(event_documents.clone()),
        query: Some(query.clone()),
        top_n,
        max_retries: base.retry_policy.max_retries,
        headers: base.headers.clone(),
        provider_options: base.provider_options.clone(),
    });

    if documents.is_empty() {
        tokio::join!(
            Hooks::emit(&hooks.on_start, start.clone()),
            telemetry.on_rerank_operation_start(&start),
        );
        let result = RerankResult {
            original_documents: documents,
            ranking: Vec::new(),
            warnings: Vec::new(),
            response: ResponseMetadata {
                timestamp: Some(Utc::now()),
                model_id: Some(identity.model_id.clone()),
                ..ResponseMetadata::default()
            },
            provider_metadata: None,
        };
        let end = Arc::new(RerankCallEndEvent {
            runtime_context: Some(hooks.runtime_context),
            call_id,
            operation_id: "ai.rerank",
            model: identity,
            documents: Some(event_documents),
            query: Some(query),
            ranking: Some(Vec::new()),
            warnings: result.warnings.clone(),
            provider_metadata: result.provider_metadata.clone(),
            response: result.response.clone(),
        });
        tokio::join!(
            Hooks::emit(&hooks.on_end, end.clone()),
            telemetry.on_rerank_operation_end(&end),
        );
        return Ok(result);
    }

    let model_documents = to_model_documents(&documents)?;
    base.run(|base, token| {
        async move {
            tokio::join!(
                Hooks::emit(&hooks.on_start, start.clone()),
                telemetry.on_rerank_operation_start(&start),
            );
            let headers = base.request_headers();
            let outcome = retry(&base.retry_policy, &token, |_| {
                let options = RerankOptions {
                    query: query.clone(),
                    documents: model_documents.clone(),
                    top_n,
                    provider_options: base.provider_options.clone(),
                    headers: headers.clone(),
                    cancellation: token.child_token(),
                };
                let model = &model;
                let telemetry = &telemetry;
                let call_id = &call_id;
                let identity = &identity;
                async move {
                    let started = Instant::now();
                    telemetry
                        .on_rerank_start(&RerankStartEvent {
                            call_id: call_id.clone(),
                            model: identity.clone(),
                            document_count: options.documents.len(),
                            query: telemetry.record_inputs().then(|| options.query.clone()),
                        })
                        .await;
                    let result = model.do_rerank(options).await.map_err(Error::from)?;
                    telemetry
                        .on_rerank_end(&RerankEndEvent {
                            call_id: call_id.clone(),
                            ranked_count: result.ranking.len(),
                            duration: started.elapsed(),
                        })
                        .await;
                    Ok(result)
                }
            })
            .await;
            let result = match outcome {
                Ok(result) => result,
                Err(error) => {
                    telemetry
                        .on_error(&ErrorEvent {
                            call_id: &call_id,
                            error: &error,
                            phase: ErrorPhase::ModelCall,
                        })
                        .await;
                    return Err(error);
                }
            };
            spans::log_warnings(&result.warnings, &identity);
            let mut ranking: Vec<Ranked<D>> = Vec::with_capacity(result.ranking.len());
            for ranked in result.ranking {
                let Some(document) = documents.get(ranked.index) else {
                    return Err(Error::from(ProviderError::InvalidResponseData(Box::new(
                        InvalidResponseDataError::new(
                            format!(
                                "ranking index {} is out of range for {} documents",
                                ranked.index,
                                documents.len()
                            ),
                            json!({ "index": ranked.index, "documents": documents.len() }),
                        ),
                    ))));
                };
                ranking.push(Ranked {
                    original_index: ranked.index,
                    score: ranked.relevance_score,
                    document: document.clone(),
                });
            }
            let mut response = result.response;
            if response.timestamp.is_none() {
                response.timestamp = Some(Utc::now());
            }
            if response.model_id.is_none() {
                response.model_id = Some(identity.model_id.clone());
            }
            let event_ranking = ranking
                .iter()
                .map(|ranked| Ranked {
                    original_index: ranked.original_index,
                    score: ranked.score,
                    document: ranked.document.clone().into(),
                })
                .collect();
            let end = Arc::new(RerankCallEndEvent {
                runtime_context: Some(hooks.runtime_context),
                call_id,
                operation_id: "ai.rerank",
                model: identity,
                documents: Some(event_documents),
                query: Some(query),
                ranking: Some(event_ranking),
                warnings: result.warnings.clone(),
                provider_metadata: result.provider_metadata.clone(),
                response: response.clone(),
            });
            tokio::join!(
                Hooks::emit(&hooks.on_end, end.clone()),
                telemetry.on_rerank_operation_end(&end),
            );
            Ok(RerankResult {
                original_documents: documents,
                ranking,
                warnings: result.warnings,
                response,
                provider_metadata: result.provider_metadata,
            })
        }
        .instrument(span)
    })
    .await
}
