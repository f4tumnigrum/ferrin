use std::collections::BTreeMap;
use std::sync::Arc;

use ferrin_core::EmbeddingModelMiddleware;
use ferrin_core::middleware::EmbeddingMiddlewareContext;
use ferrin_core::middleware::builtin::EmbeddingDefaults;
use ferrin_core::middleware::builtin::default_embedding_settings;
use ferrin_spec::DynEmbeddingModel;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::embedding_model::EmbedOptions;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::suite::modalities::common::EmbedMock;

fn object(value: serde_json::Value) -> JsonObject {
    value.as_object().cloned().unwrap()
}

fn defaults() -> EmbeddingDefaults {
    EmbeddingDefaults {
        headers: Headers::new().with("x-a", "default").with("x-b", "default"),
        provider_options: BTreeMap::from([(
            "mock".to_owned(),
            object(json!({"dimensions": 256, "nested": {"keep": true, "over": 1}})),
        )]),
    }
}

#[test]
fn call_values_take_precedence_over_defaults() {
    let middleware = default_embedding_settings(defaults());
    let mut options = EmbedOptions::new(vec!["v".to_owned()]);
    options.headers = Headers::new().with("x-b", "call");
    options
        .provider_options
        .insert("mock".to_owned(), object(json!({"nested": {"over": 2}})));
    options
        .provider_options
        .insert("other".to_owned(), object(json!({"k": 1})));

    let applied = middleware.apply(options);
    assert_eq!(applied.values, vec!["v".to_owned()]);
    assert_eq!(applied.headers.get_str("x-a"), Some("default"));
    assert_eq!(applied.headers.get_str("x-b"), Some("call"));
    assert_eq!(
        applied.provider_options,
        BTreeMap::from([
            (
                "mock".to_owned(),
                object(json!({"dimensions": 256, "nested": {"keep": true, "over": 2}})),
            ),
            ("other".to_owned(), object(json!({"k": 1}))),
        ])
    );
}

#[test]
fn empty_defaults_leave_the_call_unchanged() {
    let middleware = default_embedding_settings(EmbeddingDefaults::default());
    let mut options = EmbedOptions::new(vec!["v".to_owned()]);
    options.headers = Headers::new().with("x-b", "call");
    let applied = middleware.apply(options);
    assert_eq!(applied.headers.get_str("x-b"), Some("call"));
    assert_eq!(applied.headers.len(), 1);
    assert!(applied.provider_options.is_empty());
}

#[tokio::test]
async fn transform_params_applies_the_defaults() {
    let middleware = default_embedding_settings(defaults());
    let model = Arc::new(EmbedMock::new());
    let ctx = EmbeddingMiddlewareContext {
        model: model.as_ref() as &dyn DynEmbeddingModel,
    };
    let applied = middleware
        .transform_params(EmbedOptions::new(vec!["v".to_owned()]), ctx)
        .await
        .unwrap();
    assert_eq!(applied.headers.get_str("x-a"), Some("default"));
    assert_eq!(
        applied.provider_options["mock"],
        object(json!({"dimensions": 256, "nested": {"keep": true, "over": 1}}))
    );
}
