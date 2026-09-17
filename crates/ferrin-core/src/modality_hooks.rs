//! Operation hooks for embedding and reranking.
//!
//! Event contracts are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated to Rust and modified; see NOTICE.

use std::fmt;

use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use serde_json::json;

use crate::embed::Embedding;
use crate::embed::EmbeddingUsage;
use crate::hooks::HookList;
use crate::rerank::Ranked;
use crate::rerank::RerankDocument;
use crate::telemetry::ModelIdentity;

/// Input shape of an embedding operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EmbeddingInput {
    /// One value supplied to `embed`.
    Single(String),
    /// Values supplied to `embed_many`, in input order.
    Many(Vec<String>),
}

/// Output shape of an embedding operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EmbeddingOutput {
    /// One vector returned by `embed`.
    Single(Embedding),
    /// Vectors returned by `embed_many`, in input order.
    Many(Vec<Embedding>),
}

/// Response metadata shape of an embedding operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EmbeddingResponse {
    /// The response to `embed`.
    Single(Box<ResponseMetadata>),
    /// Responses to `embed_many`, in input chunk order.
    Many(Vec<ResponseMetadata>),
}

/// Event before any provider attempt of an embedding operation.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedCallStartEvent {
    /// Application context, absent only in restricted telemetry copies.
    pub runtime_context: Option<JsonValue>,
    /// Identifier shared by the operation and its model attempts.
    pub call_id: String,
    /// `ai.embed` or `ai.embedMany`.
    pub operation_id: &'static str,
    /// Provider and model identity.
    pub model: ModelIdentity,
    /// Original input; omitted when telemetry does not record inputs.
    pub value: Option<EmbeddingInput>,
    /// Maximum retries for each provider call.
    pub max_retries: u32,
    /// Request headers, including the core User-Agent suffix.
    pub headers: Headers,
    /// Provider-specific options.
    pub provider_options: ProviderOptions,
}

/// Event after all chunks and retries of an embedding operation succeed.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedCallEndEvent {
    /// Application context, absent only in restricted telemetry copies.
    pub runtime_context: Option<JsonValue>,
    /// Identifier shared with the start event.
    pub call_id: String,
    /// `ai.embed` or `ai.embedMany`.
    pub operation_id: &'static str,
    /// Provider and model identity.
    pub model: ModelIdentity,
    /// Original input; omitted when telemetry does not record inputs.
    pub value: Option<EmbeddingInput>,
    /// Generated vectors; omitted when telemetry does not record outputs.
    pub embedding: Option<EmbeddingOutput>,
    /// Aggregated token usage.
    pub usage: EmbeddingUsage,
    /// Warnings in input chunk order.
    pub warnings: Vec<Warning>,
    /// Provider metadata merged in input chunk order.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Provider response metadata.
    pub response: EmbeddingResponse,
}

/// Event before any provider attempt of a reranking operation.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankCallStartEvent {
    /// Application context, absent only in restricted telemetry copies.
    pub runtime_context: Option<JsonValue>,
    /// Identifier shared by the operation and its model attempts.
    pub call_id: String,
    /// `ai.rerank`.
    pub operation_id: &'static str,
    /// Provider and model identity.
    pub model: ModelIdentity,
    /// Original documents; omitted when telemetry does not record inputs.
    pub documents: Option<Vec<RerankDocument>>,
    /// Original query; omitted when telemetry does not record inputs.
    pub query: Option<String>,
    /// Requested maximum number of ranked documents.
    pub top_n: Option<usize>,
    /// Maximum retries for the provider call.
    pub max_retries: u32,
    /// Caller-provided request headers.
    pub headers: Headers,
    /// Provider-specific options.
    pub provider_options: ProviderOptions,
}

/// Event after a successful reranking operation, including an empty input.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankCallEndEvent {
    /// Application context, absent only in restricted telemetry copies.
    pub runtime_context: Option<JsonValue>,
    /// Identifier shared with the start event.
    pub call_id: String,
    /// `ai.rerank`.
    pub operation_id: &'static str,
    /// Provider and model identity.
    pub model: ModelIdentity,
    /// Original documents; omitted when telemetry does not record inputs.
    pub documents: Option<Vec<RerankDocument>>,
    /// Original query; omitted when telemetry does not record inputs.
    pub query: Option<String>,
    /// Ranked documents; omitted when telemetry does not record outputs.
    pub ranking: Option<Vec<Ranked<RerankDocument>>>,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Response metadata, including resolved timestamp and model ID.
    pub response: ResponseMetadata,
}

pub(crate) struct ModalityHooks<S, E> {
    pub(crate) runtime_context: JsonValue,
    pub(crate) on_start: HookList<S>,
    pub(crate) on_end: HookList<E>,
}

impl<S, E> Default for ModalityHooks<S, E> {
    fn default() -> Self {
        Self {
            runtime_context: json!({}),
            on_start: Vec::new(),
            on_end: Vec::new(),
        }
    }
}

impl<S, E> fmt::Debug for ModalityHooks<S, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalityHooks")
            .field("on_start", &self.on_start.len())
            .field("on_end", &self.on_end.len())
            .finish_non_exhaustive()
    }
}

macro_rules! impl_modality_hooks {
    ($ty:ident $(<$generic:ident>)?, $start:ty, $end:ty) => {
        impl$(<$generic>)? $ty$(<$generic>)? {
            /// Sets the application context passed to operation callbacks.
            #[must_use]
            pub fn runtime_context(mut self, context: ::ferrin_spec::JsonValue) -> Self {
                self.hooks.runtime_context = context;
                self
            }

            /// Adds an awaited callback before the first provider attempt.
            #[must_use]
            pub fn on_start(mut self, hook: impl $crate::hooks::HookFn<$start>) -> Self {
                self.hooks.on_start.push(::std::sync::Arc::new(hook));
                self
            }

            /// Adds an awaited callback after the entire operation succeeds.
            #[must_use]
            pub fn on_end(mut self, hook: impl $crate::hooks::HookFn<$end>) -> Self {
                self.hooks.on_end.push(::std::sync::Arc::new(hook));
                self
            }
        }
    };
}
pub(crate) use impl_modality_hooks;
