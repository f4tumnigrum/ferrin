use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::ErrorKind;
use ferrin_core::cosine_similarity;
use ferrin_core::embed;
use ferrin_core::embed_many;
use pretty_assertions::assert_eq;

use super::common::EmbedMock;
use super::common::fast_retry;

#[tokio::test]
async fn embeds_a_single_value() {
    let model = Arc::new(EmbedMock::new());
    let result = embed(Arc::clone(&model), "hello").await.unwrap();
    assert_eq!(result.value, "hello");
    assert_eq!(result.embedding, vec![5.0, 1.0]);
    assert_eq!(result.usage.tokens, Some(1));
    assert_eq!(model.calls(), vec![vec!["hello".to_owned()]]);
}

#[tokio::test]
async fn embed_many_splits_by_the_model_limits_and_keeps_order() {
    let mut mock = EmbedMock::new();
    mock.max_per_call = Some(2);
    let model = Arc::new(mock);
    let values = ["a", "bb", "ccc", "dddd", "eeeee"];
    let result = embed_many(Arc::clone(&model), values)
        .max_parallel_calls(2)
        .await
        .unwrap();
    assert_eq!(
        result.embeddings,
        vec![
            vec![1.0, 1.0],
            vec![2.0, 1.0],
            vec![3.0, 1.0],
            vec![4.0, 1.0],
            vec![5.0, 1.0]
        ]
    );
    assert_eq!(result.usage.tokens, Some(5));
    assert_eq!(result.responses.len(), 3);
    let mut calls = model.calls();
    calls.sort();
    assert_eq!(
        calls,
        vec![
            vec!["a".to_owned(), "bb".to_owned()],
            vec!["ccc".to_owned(), "dddd".to_owned()],
            vec!["eeeee".to_owned()],
        ]
    );
}

#[tokio::test]
async fn embed_many_splits_by_input_bytes() {
    let mut mock = EmbedMock::new();
    mock.max_bytes = Some(4);
    mock.parallel = false;
    let model = Arc::new(mock);
    let result = embed_many(Arc::clone(&model), ["ab", "cd", "efghij", "k"])
        .await
        .unwrap();
    assert_eq!(result.embeddings.len(), 4);
    assert_eq!(
        model.calls(),
        vec![
            vec!["ab".to_owned(), "cd".to_owned()],
            vec!["efghij".to_owned()],
            vec!["k".to_owned()],
        ]
    );
}

#[tokio::test]
async fn embed_many_retries_provider_failures() {
    let mut mock = EmbedMock::new();
    mock.fail_first = 1;
    let model = Arc::new(mock);
    let result = embed_many(Arc::clone(&model), ["a", "b"])
        .retry(fast_retry(2))
        .await
        .unwrap();
    assert_eq!(result.embeddings.len(), 2);
    assert_eq!(model.calls().len(), 2);
}

#[tokio::test]
async fn embedding_count_mismatch_is_a_provider_error() {
    let mut mock = EmbedMock::new();
    mock.drop_first = 1;
    let model = Arc::new(mock);
    let error = embed_many(model, ["a", "b"]).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Provider);
    assert!(
        error.to_string().contains("expected 2 embeddings"),
        "{error}"
    );
}

#[tokio::test]
async fn unresolved_model_ids_need_a_registry() {
    let error = embed("nowhere:model", "x").await.unwrap_err();
    assert!(matches!(error, Error::NoDefaultRegistry { .. }), "{error}");
}

#[test]
fn cosine_similarity_matches_the_definition() {
    assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < f32::EPSILON);
    assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).unwrap()).abs() < f32::EPSILON);
    assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]).unwrap(), 0.0);
    assert!(cosine_similarity(&[1.0], &[1.0, 2.0]).is_err());
}

#[tokio::test]
async fn embedding_telemetry_pairs_each_chunk_and_retry_attempt() {
    use ferrin_core::Telemetry;
    use ferrin_core::TelemetryOptions;
    use ferrin_core::telemetry::EmbedEndEvent;
    use ferrin_core::telemetry::EmbedStartEvent;
    use ferrin_core::telemetry::ErrorEvent;
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        starts: Mutex<Vec<String>>,
        ends: Mutex<Vec<String>>,
        errors: Mutex<Vec<String>>,
    }
    impl Telemetry for Recorder {
        fn on_embed_start(&self, event: &EmbedStartEvent) {
            self.starts.lock().unwrap().push(event.call_id.clone());
        }
        fn on_embed_end(&self, event: &EmbedEndEvent) {
            self.ends.lock().unwrap().push(event.call_id.clone());
        }
        fn on_error(&self, event: &ErrorEvent<'_>) {
            self.errors.lock().unwrap().push(event.call_id.to_owned());
        }
    }
    let recorder = Arc::new(Recorder::default());
    let mut model = EmbedMock::new();
    model.max_per_call = Some(1);
    model.fail_first = 1;
    let result = embed_many(Arc::new(model), ["a", "b"])
        .retry(fast_retry(2))
        .telemetry(TelemetryOptions::enabled().with_integration(recorder.clone()))
        .await
        .unwrap();
    assert_eq!(result.usage.tokens, Some(2));
    let starts = recorder.starts.lock().unwrap();
    let ends = recorder.ends.lock().unwrap();
    let errors = recorder.errors.lock().unwrap();
    assert_eq!((starts.len(), ends.len(), errors.len()), (3, 2, 1));
    let unique: BTreeSet<_> = starts.iter().collect();
    assert_eq!(unique.len(), starts.len());
    assert_eq!(unique, ends.iter().chain(errors.iter()).collect());
    let parents: BTreeSet<_> = starts
        .iter()
        .map(|id| id.split("/chunk/").next().unwrap())
        .collect();
    assert_eq!(parents.len(), 1);
}
