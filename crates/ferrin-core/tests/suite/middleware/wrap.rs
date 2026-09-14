use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::LanguageModelMiddleware;
use ferrin_core::generate_text;
use ferrin_core::middleware::GenerateNext;
use ferrin_core::middleware::MiddlewareContext;
use ferrin_core::stream_text;
use ferrin_core::wrap_language_model;
use ferrin_spec::BoxFuture;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::ModelId;
use ferrin_spec::Usage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_testing::text_parts;
use pretty_assertions::assert_eq;

use crate::suite::common::mock;
use crate::suite::common::text_model;
use crate::suite::common::text_result;

struct Marker {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
}

impl LanguageModelMiddleware for Marker {
    fn transform_params<'a>(
        &'a self,
        mut options: CallOptions,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        self.log
            .lock()
            .unwrap()
            .push(format!("{}:transform", self.name));
        options.temperature = Some(options.temperature.unwrap_or(0.0) + 1.0);
        Box::pin(async move { Ok(options) })
    }

    fn wrap_generate<'a>(
        &'a self,
        options: CallOptions,
        next: GenerateNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<GenerateResult, ProviderError>> {
        Box::pin(async move {
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:before", self.name));
            let mut result = next(options).await?;
            result
                .content
                .push(Content::text(format!("[{}]", self.name)));
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:after", self.name));
            Ok(result)
        })
    }

    fn override_model_id(&self, _model: &dyn DynLanguageModel) -> Option<ModelId> {
        (self.name == "outer").then(|| ModelId::new("overridden"))
    }
}

#[tokio::test]
async fn middleware_wraps_outermost_first() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let model = text_model("hi");
    let wrapped = wrap_language_model(
        Arc::clone(&model) as Arc<dyn DynLanguageModel>,
        [
            Arc::new(Marker {
                name: "outer",
                log: Arc::clone(&log),
            }) as Arc<dyn LanguageModelMiddleware>,
            Arc::new(Marker {
                name: "inner",
                log: Arc::clone(&log),
            }),
        ],
    );
    let result = generate_text(wrapped).prompt("x").await.unwrap();
    assert_eq!(result.text(), "hi[inner][outer]");
    assert_eq!(result.last_step().model.model_id.as_str(), "overridden");
    assert_eq!(model.generate_calls()[0].temperature, Some(2.0));
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
async fn stream_calls_pass_through_default_middleware() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let model = mock()
        .stream(text_parts(["a", "b"], Usage::totals(1, 1)))
        .generate(text_result("unused"))
        .build_shared();
    let wrapped = wrap_language_model(
        Arc::clone(&model) as Arc<dyn DynLanguageModel>,
        [Arc::new(Marker {
            name: "only",
            log: Arc::clone(&log),
        }) as Arc<dyn LanguageModelMiddleware>],
    );
    let result = stream_text(wrapped)
        .prompt("x")
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    assert_eq!(result.text(), "ab");
    assert_eq!(*log.lock().unwrap(), vec!["only:transform"]);
    assert_eq!(model.stream_calls()[0].temperature, Some(1.0));
}
