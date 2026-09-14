//! Embeddings: [`embed`] for one value, [`embed_many`] for several (split
//! into provider calls by the model's limits) and [`cosine_similarity`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §1.

use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Instant;

use ferrin_spec::BoxFuture;
use ferrin_spec::DynEmbeddingModel;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::Headers;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult as ModelEmbedResult;
pub use ferrin_spec::embedding_model::Embedding;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use serde_json::json;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::error::Error;
use crate::ids::default_id_generator;
use crate::modality::ModalityOptions;
use crate::modality::accumulate_provider_metadata;
use crate::modality::impl_modality_builder;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::RetryPolicy;
use crate::retry::retry;
use crate::telemetry::EmbedEndEvent;
use crate::telemetry::EmbedStartEvent;
use crate::telemetry::ErrorEvent;
use crate::telemetry::ErrorPhase;
use crate::telemetry::ModelIdentity;
use crate::telemetry::dispatcher::TelemetryDispatcher;
use crate::telemetry::spans;

/// Token usage of embedding calls; `None` when the provider reported none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmbeddingUsage {
    /// Input tokens consumed.
    pub tokens: Option<u64>,
}

/// Result of [`embed`].
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedResult {
    /// The embedded value.
    pub value: String,
    /// The embedding.
    pub embedding: Embedding,
    /// Token usage.
    pub usage: EmbeddingUsage,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Response metadata.
    pub response: ResponseMetadata,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Result of [`embed_many`].
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedManyResult {
    /// The embedded values, in input order.
    pub values: Vec<String>,
    /// One embedding per value, in input order.
    pub embeddings: Vec<Embedding>,
    /// Token usage summed over all calls.
    pub usage: EmbeddingUsage,
    /// Adapter warnings of all calls.
    pub warnings: Vec<Warning>,
    /// Response metadata of every call.
    pub responses: Vec<ResponseMetadata>,
    /// Provider-specific metadata merged over all calls.
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Embeds one value.
#[must_use]
pub fn embed(model: impl Into<EmbeddingModelRef>, value: impl Into<String>) -> Embed {
    Embed {
        model: model.into(),
        value: value.into(),
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`embed`]; `.await` runs the call.
#[derive(Debug)]
pub struct Embed {
    model: EmbeddingModelRef,
    value: String,
    base: ModalityOptions,
}

impl_modality_builder!(Embed);

impl IntoFuture for Embed {
    type Output = Result<EmbedResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let many = run(self.model, vec![self.value], self.base, Some(1)).await?;
            let EmbedManyResult {
                values,
                embeddings,
                usage,
                warnings,
                responses,
                provider_metadata,
            } = many;
            let (Some(value), Some(embedding), Some(response)) = (
                values.into_iter().next(),
                embeddings.into_iter().next(),
                responses.into_iter().next(),
            ) else {
                return Err(invalid_count(1, 0));
            };
            Ok(EmbedResult {
                value,
                embedding,
                usage,
                warnings,
                response,
                provider_metadata,
            })
        })
    }
}

/// Embeds several values, splitting them into provider calls by the
/// model's limits and running the calls concurrently when the model
/// allows it.
#[must_use]
pub fn embed_many(
    model: impl Into<EmbeddingModelRef>,
    values: impl IntoIterator<Item = impl Into<String>>,
) -> EmbedMany {
    EmbedMany {
        model: model.into(),
        values: values.into_iter().map(Into::into).collect(),
        max_parallel_calls: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`embed_many`]; `.await` runs the calls.
#[derive(Debug)]
pub struct EmbedMany {
    model: EmbeddingModelRef,
    values: Vec<String>,
    max_parallel_calls: Option<usize>,
    base: ModalityOptions,
}

impl EmbedMany {
    /// Limits the number of concurrent provider calls (default: unlimited;
    /// ignored when the model does not support parallel calls).
    #[must_use]
    pub fn max_parallel_calls(mut self, max_parallel_calls: usize) -> Self {
        self.max_parallel_calls = Some(max_parallel_calls.max(1));
        self
    }
}

impl_modality_builder!(EmbedMany);

impl IntoFuture for EmbedMany {
    type Output = Result<EmbedManyResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(
            self.model,
            self.values,
            self.base,
            self.max_parallel_calls,
        ))
    }
}

/// Cosine similarity of two vectors; `0.0` when either has zero length.
///
/// # Errors
///
/// Returns [`Error::InvalidArgument`] when the vectors have different
/// dimensions.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32, Error> {
    if a.len() != b.len() {
        return Err(Error::invalid_argument(
            "vectors",
            format!(
                "vectors must have the same length (got {} and {})",
                a.len(),
                b.len()
            ),
        ));
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return Ok(0.0);
    }
    Ok(dot / (norm_a * norm_b))
}

/// Splits `values` into chunks that respect both limits: a chunk is closed
/// when it already holds `max_embeddings` values or when adding the next
/// value would exceed `max_bytes`; a single oversized value still forms
/// its own chunk.
pub(crate) fn split_by_limits(
    values: &[String],
    max_embeddings: usize,
    max_bytes: usize,
) -> Vec<Vec<String>> {
    let mut chunks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_bytes = 0usize;
    for value in values {
        let bytes = value.len();
        if !current.is_empty()
            && (current.len() >= max_embeddings || current_bytes.saturating_add(bytes) > max_bytes)
        {
            chunks.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current.push(value.clone());
        current_bytes = current_bytes.saturating_add(bytes);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn invalid_count(expected: usize, received: usize) -> Error {
    Error::from(ProviderError::InvalidResponseData(Box::new(
        InvalidResponseDataError::new(
            format!("expected {expected} embeddings, received {received}"),
            json!({ "expected": expected, "received": received }),
        ),
    )))
}

/// Everything one chunk call needs (owned, so it can run on a task).
struct ChunkCall {
    model: Arc<dyn DynEmbeddingModel>,
    identity: ModelIdentity,
    values: Vec<String>,
    headers: Headers,
    provider_options: ProviderOptions,
    retry_policy: RetryPolicy,
    cancellation: CancellationToken,
    telemetry: TelemetryDispatcher,
    call_id: String,
}

impl ChunkCall {
    async fn run(self) -> Result<ModelEmbedResult, Error> {
        let Self {
            model,
            identity,
            values,
            headers,
            provider_options,
            retry_policy,
            cancellation,
            telemetry,
            call_id,
        } = self;
        let outcome = retry(&retry_policy, &cancellation, |_| {
            let values = values.clone();
            let model = &model;
            let identity = &identity;
            let headers = &headers;
            let provider_options = &provider_options;
            let cancellation = &cancellation;
            let telemetry = &telemetry;
            let call_id = &call_id;
            async move {
                let started = Instant::now();
                telemetry.on_embed_start(&EmbedStartEvent {
                    call_id: call_id.clone(),
                    model: identity.clone(),
                    value_count: values.len(),
                    values: telemetry.record_inputs().then(|| values.clone()),
                });
                let result = model
                    .do_embed(EmbedOptions {
                        values,
                        headers: headers.clone(),
                        provider_options: provider_options.clone(),
                        cancellation: cancellation.child_token(),
                    })
                    .await
                    .map_err(Error::from)?;
                telemetry.on_embed_end(&EmbedEndEvent {
                    call_id: call_id.clone(),
                    embedding_count: result.embeddings.len(),
                    tokens: result.usage.map(|usage| usage.tokens),
                    duration: started.elapsed(),
                });
                Ok(result)
            }
        })
        .await;
        let result = match outcome {
            Ok(result) => result,
            Err(error) => {
                telemetry.on_error(&ErrorEvent {
                    call_id: &call_id,
                    error: &error,
                    phase: ErrorPhase::ModelCall,
                });
                return Err(error);
            }
        };
        if result.embeddings.len() != values.len() {
            return Err(invalid_count(values.len(), result.embeddings.len()));
        }
        spans::log_warnings(&result.warnings, &identity);
        Ok(result)
    }
}

async fn run(
    model: EmbeddingModelRef,
    values: Vec<String>,
    base: ModalityOptions,
    max_parallel_calls: Option<usize>,
) -> Result<EmbedManyResult, Error> {
    let model = resolve_model(&model, ProviderRegistry::embedding_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("embed", &identity);
    base.run(|base, token| {
        async move { run_calls(model, identity, values, &base, max_parallel_calls, token).await }
            .instrument(span)
    })
    .await
}

async fn run_calls(
    model: Arc<dyn DynEmbeddingModel>,
    identity: ModelIdentity,
    values: Vec<String>,
    base: &ModalityOptions,
    max_parallel_calls: Option<usize>,
    cancellation: CancellationToken,
) -> Result<EmbedManyResult, Error> {
    let telemetry = TelemetryDispatcher::new(base.telemetry.clone());
    let call_id = default_id_generator().generate();
    let headers = base.request_headers();

    let max_embeddings = match model.max_embeddings_per_call() {
        Some(0) => {
            return Err(Error::invalid_argument(
                "max_embeddings_per_call",
                "must be greater than 0",
            ));
        }
        Some(limit) => limit,
        None => usize::MAX,
    };
    let max_bytes = match model.max_input_bytes_per_call() {
        Some(0) => {
            return Err(Error::invalid_argument(
                "max_input_bytes_per_call",
                "must be greater than 0",
            ));
        }
        Some(limit) => limit,
        None => usize::MAX,
    };
    let chunks = split_by_limits(&values, max_embeddings, max_bytes);
    let parallel = if model.supports_parallel_calls() {
        max_parallel_calls.unwrap_or(usize::MAX).max(1)
    } else {
        1
    };
    let make_call = |values: Vec<String>| ChunkCall {
        model: Arc::clone(&model),
        identity: identity.clone(),
        values,
        headers: headers.clone(),
        provider_options: base.provider_options.clone(),
        retry_policy: base.retry_policy.clone(),
        cancellation: cancellation.clone(),
        telemetry: telemetry.clone(),
        call_id: call_id.clone(),
    };

    let mut results: Vec<Option<ModelEmbedResult>> = (0..chunks.len()).map(|_| None).collect();
    let indexed: Vec<(usize, Vec<String>)> = chunks.into_iter().enumerate().collect();
    for window in indexed.chunks(parallel) {
        if let [(index, values)] = window {
            let result = make_call(values.clone()).run().await?;
            if let Some(slot) = results.get_mut(*index) {
                *slot = Some(result);
            }
            continue;
        }
        let mut tasks: JoinSet<(usize, Result<ModelEmbedResult, Error>)> = JoinSet::new();
        for (index, values) in window {
            let call = make_call(values.clone());
            let index = *index;
            tasks.spawn(async move { (index, call.run().await) });
        }
        while let Some(joined) = tasks.join_next().await {
            let (index, result) = joined
                .map_err(|error| Error::message(format!("embedding task failed: {error}")))?;
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result?);
            }
        }
    }

    let mut embeddings: Vec<Embedding> = Vec::with_capacity(values.len());
    let mut warnings: Vec<Warning> = Vec::new();
    let mut responses: Vec<ResponseMetadata> = Vec::new();
    let mut tokens: Option<u64> = Some(0);
    let mut provider_metadata: Option<ProviderMetadata> = None;
    for result in results.into_iter().flatten() {
        embeddings.extend(result.embeddings);
        warnings.extend(result.warnings);
        responses.push(result.response);
        tokens = match (tokens, result.usage) {
            (Some(total), Some(usage)) => Some(total.saturating_add(usage.tokens)),
            _ => None,
        };
        accumulate_provider_metadata(&mut provider_metadata, result.provider_metadata.as_ref());
    }
    if embeddings.len() != values.len() {
        return Err(invalid_count(values.len(), embeddings.len()));
    }
    Ok(EmbedManyResult {
        values,
        embeddings,
        usage: EmbeddingUsage { tokens },
        warnings,
        responses,
        provider_metadata,
    })
}
