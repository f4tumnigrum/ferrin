//! Embeddings API.

use ferrin_spec::EmbeddingModel;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[tokio::test]
async fn embedding_json_keeps_reference_number_precision() {
    let test = TestProvider::start().await;
    let values = vec![0.123_456_789_012_345_66_f64, 1e100, 1e-100];
    test.mount_fixture(
        Method::POST,
        "/v1/embeddings",
        ferrin_testing::Fixture::json(&json!({
            "data":[{"index":0,"embedding":values}], "usage":{"prompt_tokens":1,"total_tokens":1}
        })),
    );
    let result = test
        .provider
        .embedding("model")
        .do_embed(EmbedOptions::new(vec!["input".into()]))
        .await
        .unwrap();
    assert_eq!(result.embeddings, vec![values]);
}

#[tokio::test]
async fn embed_maps_vectors_and_usage() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/embeddings", "embedding", "basic");
    let model = test.provider.embedding("text-embedding-3-small");
    assert_eq!(model.max_embeddings_per_call(), Some(2048));
    assert!(model.supports_parallel_calls());
    let mut options = EmbedOptions::new(vec!["a".to_owned(), "b".to_owned()]);
    options.provider_options = openai_options(json!({"dimensions": 3, "user": "u1"}));
    let result = model.do_embed(options).await.unwrap();
    assert_eq!(
        result.embeddings,
        vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]
    );
    assert_eq!(result.usage.unwrap().tokens, 7);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "text-embedding-3-small",
            "input": ["a", "b"],
            "encoding_format": "float",
            "dimensions": 3,
            "user": "u1"
        })
    );
}

#[tokio::test]
async fn too_many_values_are_rejected_before_the_request() {
    let test = TestProvider::start().await;
    let model = test.provider.embedding("text-embedding-3-small");
    let values = vec!["x".to_owned(); 2049];
    let error = model.do_embed(EmbedOptions::new(values)).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::TooManyEmbeddingValues(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
