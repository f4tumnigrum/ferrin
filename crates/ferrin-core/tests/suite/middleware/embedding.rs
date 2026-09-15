use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::EmbeddingModelMiddleware;
use ferrin_core::embed_many;
use ferrin_core::middleware::EmbedNext;
use ferrin_core::middleware::EmbeddingMiddlewareContext;
use ferrin_core::wrap_embedding_model;
use ferrin_spec::BoxFuture;
use ferrin_spec::DynEmbeddingModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;

use crate::suite::modalities::common::EmbedMock;

/// Logs its calls, tags every value with `[name]`, and (for `outer`) renames
/// the model and caps the batch size.
struct Marker {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
    max_per_call: Option<usize>,
}

impl EmbeddingModelMiddleware for Marker {
    fn transform_params<'a>(
        &'a self,
        mut options: EmbedOptions,
        _ctx: EmbeddingMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<EmbedOptions, ProviderError>> {
        self.log
            .lock()
            .unwrap()
            .push(format!("{}:transform", self.name));
        for value in &mut options.values {
            value.push_str(&format!("[{}]", self.name));
        }
        Box::pin(async move { Ok(options) })
    }

    fn wrap_embed<'a>(
        &'a self,
        options: EmbedOptions,
        next: EmbedNext<'a>,
        _ctx: EmbeddingMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<EmbedResult, ProviderError>> {
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

    fn override_provider(&self, _model: &dyn DynEmbeddingModel) -> Option<ProviderId> {
        (self.name == "outer").then(|| ProviderId::new("wrapped"))
    }

    fn override_model_id(&self, _model: &dyn DynEmbeddingModel) -> Option<ModelId> {
        (self.name == "outer").then(|| ModelId::new("overridden"))
    }

    fn max_embeddings_per_call(&self, model: &dyn DynEmbeddingModel) -> Option<usize> {
        self.max_per_call
            .or_else(|| model.max_embeddings_per_call())
    }

    fn supports_parallel_calls(&self, model: &dyn DynEmbeddingModel) -> bool {
        self.name != "inner" && model.supports_parallel_calls()
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

#[tokio::test]
async fn embedding_middleware_wraps_outermost_first() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mock = Arc::new(EmbedMock::new());
    let wrapped = wrap_embedding_model(
        Arc::clone(&mock) as Arc<dyn DynEmbeddingModel>,
        [
            Arc::new(marker("outer", &log, Some(2))) as Arc<dyn EmbeddingModelMiddleware>,
            Arc::new(marker("inner", &log, None)),
        ],
    );
    assert_eq!(wrapped.provider().as_str(), "wrapped");
    assert_eq!(wrapped.model_id().as_str(), "overridden");
    assert_eq!(wrapped.max_embeddings_per_call(), Some(2));
    assert_eq!(wrapped.max_input_bytes_per_call(), None);
    assert!(!wrapped.supports_parallel_calls());

    let result = wrapped
        .do_embed(EmbedOptions::new(vec!["a".to_owned()]))
        .await
        .unwrap();
    assert_eq!(result.embeddings, vec![vec![15.0, 1.0]]);
    assert_eq!(mock.calls(), vec![vec!["a[outer][inner]".to_owned()]]);
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
async fn embed_many_splits_by_the_middleware_limit() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mock = Arc::new(EmbedMock::new());
    let wrapped = wrap_embedding_model(
        Arc::clone(&mock) as Arc<dyn DynEmbeddingModel>,
        [Arc::new(marker("limit", &log, Some(2))) as Arc<dyn EmbeddingModelMiddleware>],
    );
    let result = embed_many(wrapped, ["a", "bb", "ccc"]).await.unwrap();
    assert_eq!(
        result.embeddings,
        vec![vec![8.0, 1.0], vec![9.0, 1.0], vec![10.0, 1.0]]
    );
    assert_eq!(result.responses.len(), 2);
    let mut calls = mock.calls();
    calls.sort();
    assert_eq!(
        calls,
        vec![
            vec!["a[limit]".to_owned(), "bb[limit]".to_owned()],
            vec!["ccc[limit]".to_owned()],
        ]
    );
    assert_eq!(log.lock().unwrap().len(), 6);
}

#[test]
fn empty_middleware_returns_the_model() {
    let model = Arc::new(EmbedMock::new()) as Arc<dyn DynEmbeddingModel>;
    let same = wrap_embedding_model(
        Arc::clone(&model),
        Vec::<Arc<dyn EmbeddingModelMiddleware>>::new(),
    );
    assert!(Arc::ptr_eq(&model, &same));
}
