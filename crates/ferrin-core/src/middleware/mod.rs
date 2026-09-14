//! Language model middleware.
//!
//! A middleware wraps a model: it may rewrite call options, wrap the
//! generate/stream calls, and override identity or supported URLs. Apply
//! with [`wrap_language_model`]; the first middleware in the list is the
//! outermost.

use std::fmt;

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::GenerateResult;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::StreamResult;
use ferrin_spec::SupportedUrls;
use ferrin_spec::error::ProviderError;

pub mod builtin;
mod wrap;

pub use wrap::wrap_language_model;

/// Which model method is being called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallKind {
    /// `do_generate`.
    Generate,
    /// `do_stream`.
    Stream,
}

/// The model being wrapped and the kind of call.
#[derive(Clone, Copy)]
pub struct MiddlewareContext<'a> {
    /// The wrapped (inner) model.
    pub model: &'a dyn DynLanguageModel,
    /// The kind of call.
    pub kind: CallKind,
}

impl fmt::Debug for MiddlewareContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MiddlewareContext")
            .field("provider", self.model.provider())
            .field("model_id", self.model.model_id())
            .field("kind", &self.kind)
            .finish()
    }
}

/// Continuation of a wrapped `do_generate`.
pub type GenerateNext<'a> = Box<
    dyn FnOnce(CallOptions) -> BoxFuture<'a, Result<GenerateResult, ProviderError>> + Send + 'a,
>;

/// Continuation of a wrapped `do_stream`.
pub type StreamNext<'a> =
    Box<dyn FnOnce(CallOptions) -> BoxFuture<'a, Result<StreamResult, ProviderError>> + Send + 'a>;

/// Intercepts language model calls. Every method has a pass-through default.
pub trait LanguageModelMiddleware: Send + Sync + 'static {
    /// Rewrites the call options before the call.
    fn transform_params<'a>(
        &'a self,
        options: CallOptions,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        Box::pin(async move { Ok(options) })
    }

    /// Wraps `do_generate`.
    fn wrap_generate<'a>(
        &'a self,
        options: CallOptions,
        next: GenerateNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<GenerateResult, ProviderError>> {
        next(options)
    }

    /// Wraps `do_stream`.
    fn wrap_stream<'a>(
        &'a self,
        options: CallOptions,
        next: StreamNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<StreamResult, ProviderError>> {
        next(options)
    }

    /// Overrides the provider id reported by the wrapped model.
    fn override_provider(&self, _model: &dyn DynLanguageModel) -> Option<ProviderId> {
        None
    }

    /// Overrides the model id reported by the wrapped model.
    fn override_model_id(&self, _model: &dyn DynLanguageModel) -> Option<ModelId> {
        None
    }

    /// Overrides the supported URLs of the wrapped model.
    fn override_supported_urls<'a>(
        &'a self,
        _model: &'a dyn DynLanguageModel,
    ) -> Option<BoxFuture<'a, SupportedUrls>> {
        None
    }
}
