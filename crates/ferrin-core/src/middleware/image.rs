//! Image model middleware.
//!
//! Mirrors the language model middleware for [`ImageModel`]: a layer may
//! rewrite the image options, wrap `do_generate`, and override the identity
//! or per-call limit reported by the wrapped model. Apply with
//! [`wrap_image_model`]; the first middleware in the list is the outermost.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::DynImageModel;
use ferrin_spec::ImageModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;

/// The image model being wrapped.
#[derive(Clone, Copy)]
pub struct ImageMiddlewareContext<'a> {
    /// The wrapped (inner) model.
    pub model: &'a dyn DynImageModel,
}

impl fmt::Debug for ImageMiddlewareContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageMiddlewareContext")
            .field("provider", self.model.provider())
            .field("model_id", self.model.model_id())
            .finish()
    }
}

/// Continuation of a wrapped image `do_generate`.
pub type ImageGenerateNext<'a> =
    Box<dyn FnOnce(ImageOptions) -> BoxFuture<'a, Result<ImageResult, ProviderError>> + Send + 'a>;

/// Intercepts image model calls. Every method has a pass-through default.
///
/// [`max_images_per_call`](Self::max_images_per_call) receives the wrapped
/// model and returns the value the wrapper reports; its default forwards the
/// inner model's value.
pub trait ImageModelMiddleware: Send + Sync + 'static {
    /// Rewrites the image options before the call.
    fn transform_params<'a>(
        &'a self,
        options: ImageOptions,
        _ctx: ImageMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<ImageOptions, ProviderError>> {
        Box::pin(async move { Ok(options) })
    }

    /// Wraps `do_generate`.
    fn wrap_generate<'a>(
        &'a self,
        options: ImageOptions,
        next: ImageGenerateNext<'a>,
        _ctx: ImageMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<ImageResult, ProviderError>> {
        next(options)
    }

    /// Overrides the provider id reported by the wrapped model.
    fn override_provider(&self, _model: &dyn DynImageModel) -> Option<ProviderId> {
        None
    }

    /// Overrides the model id reported by the wrapped model.
    fn override_model_id(&self, _model: &dyn DynImageModel) -> Option<ModelId> {
        None
    }

    /// The per-call image limit reported by the wrapper; defaults to the
    /// inner value.
    fn max_images_per_call(&self, model: &dyn DynImageModel) -> Option<usize> {
        model.max_images_per_call()
    }
}

/// Wraps `model` with `middleware`; the first entry becomes the outermost
/// layer. An empty list returns `model` unchanged.
#[must_use]
pub fn wrap_image_model(
    model: Arc<dyn DynImageModel>,
    middleware: impl IntoIterator<Item = Arc<dyn ImageModelMiddleware>, IntoIter: DoubleEndedIterator>,
) -> Arc<dyn DynImageModel> {
    middleware.into_iter().rev().fold(model, |inner, layer| {
        let provider = layer
            .override_provider(inner.as_ref())
            .unwrap_or_else(|| inner.provider().clone());
        let model_id = layer
            .override_model_id(inner.as_ref())
            .unwrap_or_else(|| inner.model_id().clone());
        Arc::new(WrappedImageModel {
            inner,
            layer,
            provider,
            model_id,
        })
    })
}

struct WrappedImageModel {
    inner: Arc<dyn DynImageModel>,
    layer: Arc<dyn ImageModelMiddleware>,
    provider: ProviderId,
    model_id: ModelId,
}

impl fmt::Debug for WrappedImageModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrappedImageModel")
            .field("provider", &self.provider)
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl ImageModel for WrappedImageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Option<usize> {
        self.layer.max_images_per_call(self.inner.as_ref())
    }

    async fn do_generate(&self, options: ImageOptions) -> Result<ImageResult, ProviderError> {
        let ctx = ImageMiddlewareContext {
            model: self.inner.as_ref(),
        };
        let options = self.layer.transform_params(options, ctx).await?;
        let inner = &self.inner;
        self.layer
            .wrap_generate(
                options,
                Box::new(move |options| inner.do_generate(options)),
                ctx,
            )
            .await
    }
}
