//! Embedding model middleware.
//!
//! Mirrors the language model middleware for [`EmbeddingModel`]: a layer may
//! rewrite the embed options, wrap `do_embed`, and override the identity or
//! batching limits reported by the wrapped model. Apply with
//! [`wrap_embedding_model`]; the first middleware in the list is the
//! outermost.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::DynEmbeddingModel;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::error::ProviderError;

/// The embedding model being wrapped.
#[derive(Clone, Copy)]
pub struct EmbeddingMiddlewareContext<'a> {
    /// The wrapped (inner) model.
    pub model: &'a dyn DynEmbeddingModel,
}

impl fmt::Debug for EmbeddingMiddlewareContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddingMiddlewareContext")
            .field("provider", self.model.provider())
            .field("model_id", self.model.model_id())
            .finish()
    }
}

/// Continuation of a wrapped `do_embed`.
pub type EmbedNext<'a> =
    Box<dyn FnOnce(EmbedOptions) -> BoxFuture<'a, Result<EmbedResult, ProviderError>> + Send + 'a>;

/// Intercepts embedding model calls. Every method has a pass-through default.
///
/// The limit methods receive the wrapped model and return the value the
/// wrapper reports; their defaults forward the inner model's values, so a
/// layer overrides a limit by returning something else.
pub trait EmbeddingModelMiddleware: Send + Sync + 'static {
    /// Rewrites the embed options before the call.
    fn transform_params<'a>(
        &'a self,
        options: EmbedOptions,
        _ctx: EmbeddingMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<EmbedOptions, ProviderError>> {
        Box::pin(async move { Ok(options) })
    }

    /// Wraps `do_embed`.
    fn wrap_embed<'a>(
        &'a self,
        options: EmbedOptions,
        next: EmbedNext<'a>,
        _ctx: EmbeddingMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<EmbedResult, ProviderError>> {
        next(options)
    }

    /// Overrides the provider id reported by the wrapped model.
    fn override_provider(&self, _model: &dyn DynEmbeddingModel) -> Option<ProviderId> {
        None
    }

    /// Overrides the model id reported by the wrapped model.
    fn override_model_id(&self, _model: &dyn DynEmbeddingModel) -> Option<ModelId> {
        None
    }

    /// The batch limit reported by the wrapper; defaults to the inner value.
    fn max_embeddings_per_call(&self, model: &dyn DynEmbeddingModel) -> Option<usize> {
        model.max_embeddings_per_call()
    }

    /// The input byte limit reported by the wrapper; defaults to the inner
    /// value.
    fn max_input_bytes_per_call(&self, model: &dyn DynEmbeddingModel) -> Option<usize> {
        model.max_input_bytes_per_call()
    }

    /// Whether the wrapper allows concurrent calls; defaults to the inner
    /// value.
    fn supports_parallel_calls(&self, model: &dyn DynEmbeddingModel) -> bool {
        model.supports_parallel_calls()
    }
}

/// Wraps `model` with `middleware`; the first entry becomes the outermost
/// layer. An empty list returns `model` unchanged.
#[must_use]
pub fn wrap_embedding_model(
    model: Arc<dyn DynEmbeddingModel>,
    middleware: impl IntoIterator<
        Item = Arc<dyn EmbeddingModelMiddleware>,
        IntoIter: DoubleEndedIterator,
    >,
) -> Arc<dyn DynEmbeddingModel> {
    middleware.into_iter().rev().fold(model, |inner, layer| {
        let provider = layer
            .override_provider(inner.as_ref())
            .unwrap_or_else(|| inner.provider().clone());
        let model_id = layer
            .override_model_id(inner.as_ref())
            .unwrap_or_else(|| inner.model_id().clone());
        Arc::new(WrappedEmbeddingModel {
            inner,
            layer,
            provider,
            model_id,
        })
    })
}

struct WrappedEmbeddingModel {
    inner: Arc<dyn DynEmbeddingModel>,
    layer: Arc<dyn EmbeddingModelMiddleware>,
    provider: ProviderId,
    model_id: ModelId,
}

impl fmt::Debug for WrappedEmbeddingModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrappedEmbeddingModel")
            .field("provider", &self.provider)
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl EmbeddingModel for WrappedEmbeddingModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        self.layer.max_embeddings_per_call(self.inner.as_ref())
    }

    fn max_input_bytes_per_call(&self) -> Option<usize> {
        self.layer.max_input_bytes_per_call(self.inner.as_ref())
    }

    fn supports_parallel_calls(&self) -> bool {
        self.layer.supports_parallel_calls(self.inner.as_ref())
    }

    async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
        let ctx = EmbeddingMiddlewareContext {
            model: self.inner.as_ref(),
        };
        let options = self.layer.transform_params(options, ctx).await?;
        let inner = &self.inner;
        self.layer
            .wrap_embed(
                options,
                Box::new(move |options| inner.do_embed(options)),
                ctx,
            )
            .await
    }
}
