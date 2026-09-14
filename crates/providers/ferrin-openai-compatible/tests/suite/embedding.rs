//! Embeddings.

use ferrin_spec::EmbeddingModel;
use ferrin_spec::Warning;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::example_options;
use super::common::options_under;

#[tokio::test]
async fn embed_maps_vectors_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/embeddings", "embedding", "basic");
    let model = test.provider.embedding("example-embed");
    assert_eq!(model.max_embeddings_per_call(), Some(2048));
    assert!(model.supports_parallel_calls());
    let mut options = EmbedOptions::new(vec!["a".to_owned(), "b".to_owned()]);
    options.provider_options = example_options(json!({"dimensions": 3, "user": "u1"}));
    options
        .provider_options
        .extend(options_under("openai-compatible", json!({})));
    let result = model.do_embed(options).await.unwrap();
    assert_eq!(
        result.embeddings,
        vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]
    );
    assert_eq!(result.usage.unwrap().tokens, 7);
    assert_eq!(
        result.provider_metadata.unwrap()["example"]["shard"],
        json!("eu-1")
    );
    assert_eq!(
        result.warnings,
        vec![Warning::deprecated(
            "providerOptions key 'openai-compatible'",
            "Use 'openaiCompatible' instead."
        )]
    );
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("example-embed")
    );
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "example-embed",
            "input": ["a", "b"],
            "encoding_format": "float",
            "dimensions": 3,
            "user": "u1"
        })
    );
}

#[tokio::test]
async fn configured_limits_are_enforced_before_the_request() {
    let test = TestProvider::start_with(|mut settings| {
        settings.max_embeddings_per_call = Some(2);
        settings.supports_parallel_calls = Some(false);
        settings
    })
    .await;
    let model = test.provider.embedding("example-embed");
    assert_eq!(model.max_embeddings_per_call(), Some(2));
    assert!(!model.supports_parallel_calls());
    let values = vec!["x".to_owned(); 3];
    let error = model.do_embed(EmbedOptions::new(values)).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::TooManyEmbeddingValues(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
