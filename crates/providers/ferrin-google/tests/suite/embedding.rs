//! Embedding model: `embedContent` and `batchEmbedContents`.

use ferrin_spec::EmbeddingModel;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::google_options;

#[tokio::test]
async fn single_value_uses_embed_content() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/text-embedding-004:embedContent",
        "embedding",
        "single",
    );
    let model = test.provider.embedding("text-embedding-004");
    assert_eq!(model.max_embeddings_per_call(), Some(100));
    assert!(model.supports_parallel_calls());
    let mut options = EmbedOptions::new(vec!["hello".to_owned()]);
    options.provider_options = google_options(json!({
        "outputDimensionality": 3,
        "taskType": "SEMANTIC_SIMILARITY"
    }));
    let result = model.do_embed(options).await.unwrap();
    assert_eq!(result.embeddings, vec![vec![0.1, 0.2, 0.3]]);
    assert!(result.usage.is_none());
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "models/text-embedding-004",
            "content": {"parts": [{"text": "hello"}]},
            "outputDimensionality": 3,
            "taskType": "SEMANTIC_SIMILARITY"
        })
    );
}

#[tokio::test]
async fn multiple_values_use_batch_embed_contents() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/gemini-embedding-001:batchEmbedContents",
        "embedding",
        "batch",
    );
    let mut options = EmbedOptions::new(vec!["a".to_owned(), "b".to_owned()]);
    options.provider_options = google_options(json!({
        "content": [[{"inlineData": {"mimeType": "image/png", "data": "AAAA"}}], null]
    }));
    let result = test
        .provider
        .embedding("gemini-embedding-001")
        .do_embed(options)
        .await
        .unwrap();
    assert_eq!(result.embeddings.len(), 2);
    assert_eq!(result.embeddings[1], vec![0.4, 0.5, 0.6]);
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("embedding_batch_request", request);
}

#[tokio::test]
async fn too_many_values_and_mismatched_content_are_rejected() {
    let test = TestProvider::start().await;
    let values: Vec<String> = (0..101).map(|i| i.to_string()).collect();
    let error = test
        .provider
        .embedding("text-embedding-004")
        .do_embed(EmbedOptions::new(values))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::TooManyEmbeddingValues(_)),
        "{error:?}"
    );
    let mut options = EmbedOptions::new(vec!["a".to_owned(), "b".to_owned()]);
    options.provider_options = google_options(json!({"content": [null]}));
    let error = test
        .provider
        .embedding("text-embedding-004")
        .do_embed(options)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
