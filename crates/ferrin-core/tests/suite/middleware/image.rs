use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::ImageModelMiddleware;
use ferrin_core::generate_image;
use ferrin_core::middleware::ImageGenerateNext;
use ferrin_core::middleware::ImageMiddlewareContext;
use ferrin_core::wrap_image_model;
use ferrin_spec::BoxFuture;
use ferrin_spec::DynImageModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use pretty_assertions::assert_eq;

use crate::suite::modalities::common::ImageMock;
use crate::suite::modalities::common::lock;

/// Logs its calls, sets the seed, and (for `outer`) renames the model and
/// caps the per-call image count.
struct Marker {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
    max_per_call: Option<usize>,
}

impl ImageModelMiddleware for Marker {
    fn transform_params<'a>(
        &'a self,
        mut options: ImageOptions,
        _ctx: ImageMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<ImageOptions, ProviderError>> {
        self.log
            .lock()
            .unwrap()
            .push(format!("{}:transform", self.name));
        options.seed = Some(options.seed.unwrap_or(0) + 7);
        Box::pin(async move { Ok(options) })
    }

    fn wrap_generate<'a>(
        &'a self,
        options: ImageOptions,
        next: ImageGenerateNext<'a>,
        _ctx: ImageMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<ImageResult, ProviderError>> {
        Box::pin(async move {
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:before", self.name));
            let result = next(options).await?;
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:after", self.name));
            Ok(result)
        })
    }

    fn override_provider(&self, _model: &dyn DynImageModel) -> Option<ProviderId> {
        (self.name == "outer").then(|| ProviderId::new("wrapped"))
    }

    fn override_model_id(&self, _model: &dyn DynImageModel) -> Option<ModelId> {
        (self.name == "outer").then(|| ModelId::new("overridden"))
    }

    fn max_images_per_call(&self, model: &dyn DynImageModel) -> Option<usize> {
        self.max_per_call.or_else(|| model.max_images_per_call())
    }
}

fn marker(
    name: &'static str,
    log: &Arc<Mutex<Vec<String>>>,
    max_per_call: Option<usize>,
) -> Marker {
    Marker {
        name,
        log: Arc::clone(log),
        max_per_call,
    }
}

fn seeds(mock: &ImageMock) -> Vec<Option<u64>> {
    lock(&mock.calls)
        .iter()
        .map(|options| options.seed)
        .collect()
}

#[tokio::test]
async fn image_middleware_wraps_outermost_first() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = Arc::new(ImageMock::new());
    let wrapped = wrap_image_model(
        Arc::clone(&mock) as Arc<dyn DynImageModel>,
        [
            Arc::new(marker("outer", &log, Some(2))) as Arc<dyn ImageModelMiddleware>,
            Arc::new(marker("inner", &log, None)),
        ],
    );
    assert_eq!(wrapped.provider().as_str(), "wrapped");
    assert_eq!(wrapped.model_id().as_str(), "overridden");
    assert_eq!(wrapped.max_images_per_call(), Some(2));

    let result = wrapped.do_generate(ImageOptions::new("x")).await.unwrap();
    assert_eq!(result.images.len(), 1);
    assert_eq!(seeds(&mock), vec![Some(14)]);
    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "outer:transform",
            "outer:before",
            "inner:transform",
            "inner:before",
            "inner:after",
            "outer:after",
        ]
    );
}

#[tokio::test]
async fn generate_image_splits_by_the_middleware_limit() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mock = Arc::new(ImageMock::new());
    let wrapped = wrap_image_model(
        Arc::clone(&mock) as Arc<dyn DynImageModel>,
        [Arc::new(marker("limit", &log, Some(2))) as Arc<dyn ImageModelMiddleware>],
    );
    let result = generate_image(wrapped, "x").n(3).await.unwrap();
    assert_eq!(result.images.len(), 3);
    assert_eq!(result.calls.len(), 2);
    let mut counts = mock.call_counts();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, 2]);
    assert_eq!(seeds(&mock), vec![Some(7), Some(7)]);
    assert_eq!(log.lock().unwrap().len(), 6);
}

#[test]
fn empty_middleware_returns_the_model() {
    let model = Arc::new(ImageMock::new()) as Arc<dyn DynImageModel>;
    let same = wrap_image_model(
        Arc::clone(&model),
        Vec::<Arc<dyn ImageModelMiddleware>>::new(),
    );
    assert!(Arc::ptr_eq(&model, &same));
}
