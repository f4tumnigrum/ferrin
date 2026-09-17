use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Error;
use ferrin_core::ErrorKind;
use ferrin_core::cosine_similarity;
use ferrin_core::embed;
use ferrin_core::embed::EmbedCallEndEvent;
use ferrin_core::embed::EmbedCallStartEvent;
use ferrin_core::embed::EmbeddingInput;
use ferrin_core::embed::EmbeddingOutput;
use ferrin_core::embed::EmbeddingResponse;
use ferrin_core::embed_many;
use ferrin_core::telemetry::ModelIdentity;
use ferrin_core::telemetry::Telemetry;
use ferrin_core::telemetry::TelemetryOptions;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::ProviderOptions;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::EmbedMock;
use super::common::fast_retry;
use super::common::lock;

#[derive(Default)]
struct OperationRecorder {
    starts: Mutex<Vec<EmbedCallStartEvent>>,
    ends: Mutex<Vec<EmbedCallEndEvent>>,
}

impl Telemetry for OperationRecorder {
    fn on_embed_operation_start<'a>(&'a self, event: &'a EmbedCallStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.starts).push(event.clone());
        })
    }

    fn on_embed_operation_end<'a>(&'a self, event: &'a EmbedCallEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.ends).push(event.clone());
        })
    }
}

#[tokio::test]
async fn operation_telemetry_filters_copies_without_changing_embedding_hooks() {
    for record in [false, true] {
        let application = Arc::new(OperationRecorder::default());
        let integration = Arc::new(OperationRecorder::default());
        embed(EmbedMock::new(), "private input")
            .runtime_context(json!({"private": true}))
            .telemetry(TelemetryOptions {
                enabled: true,
                record_inputs: record,
                record_outputs: record,
                include_runtime_context: record,
                integrations: vec![integration.clone()],
                ..Default::default()
            })
            .on_start({
                let application = application.clone();
                move |event: Arc<EmbedCallStartEvent>| {
                    let application = application.clone();
                    async move {
                        lock(&application.starts).push((*event).clone());
                    }
                }
            })
            .on_end({
                let application = application.clone();
                move |event: Arc<EmbedCallEndEvent>| {
                    let application = application.clone();
                    async move {
                        lock(&application.ends).push((*event).clone());
                    }
                }
            })
            .await
            .unwrap();
        let mut start = lock(&application.starts)[0].clone();
        let mut end = lock(&application.ends)[0].clone();
        assert!(start.value.is_some());
        assert!(end.embedding.is_some());
        if !record {
            start.runtime_context = None;
            start.value = None;
            start.headers = Default::default();
            start.provider_options = Default::default();
            end.runtime_context = None;
            end.value = None;
            end.embedding = None;
            end.provider_metadata = None;
        }
        assert_eq!(*lock(&integration.starts), vec![start]);
        assert_eq!(*lock(&integration.ends), vec![end]);
    }
}

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
    assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < f64::EPSILON);
    assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).unwrap()).abs() < f64::EPSILON);
    assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]).unwrap(), 0.0);
    assert!(cosine_similarity(&[1.0], &[1.0, 2.0]).is_err());
}

#[test]
fn cosine_similarity_uses_reference_double_precision() {
    let value = cosine_similarity(&[1.0, 1.0], &[1.0, 0.0]).unwrap();
    assert!((value - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
    assert_eq!(
        cosine_similarity(&[1e100, 0.0], &[1e100, 0.0]).unwrap(),
        1.0
    );
    assert_eq!(
        cosine_similarity(&[1e-100, 0.0], &[1e-100, 0.0]).unwrap(),
        1.0
    );
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
        fn on_embed_start<'a>(&'a self, event: &'a EmbedStartEvent) -> BoxFuture<'a, ()> {
            self.starts.lock().unwrap().push(event.call_id.clone());
            Box::pin(async {})
        }
        fn on_embed_end<'a>(&'a self, event: &'a EmbedEndEvent) -> BoxFuture<'a, ()> {
            self.ends.lock().unwrap().push(event.call_id.clone());
            Box::pin(async {})
        }
        fn on_error<'a>(&'a self, event: &'a ErrorEvent<'a>) -> BoxFuture<'a, ()> {
            self.errors.lock().unwrap().push(event.call_id.to_owned());
            Box::pin(async {})
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

#[tokio::test]
async fn embedding_operation_hooks_wrap_all_chunks_and_retries_once() {
    let mut mock = EmbedMock::new();
    mock.max_per_call = Some(1);
    mock.fail_first = 1;
    mock.parallel = false;
    let model = Arc::new(mock);
    let starts = Arc::new(Mutex::new(Vec::new()));
    let ends = Arc::new(Mutex::new(Vec::new()));
    let result = embed_many(Arc::clone(&model), ["a", "bb"])
        .retry(fast_retry(1))
        .runtime_context(json!({"request": "r-1"}))
        .header("x-request", "r-1")
        .on_start({
            let starts = Arc::clone(&starts);
            let model = Arc::clone(&model);
            move |event: Arc<EmbedCallStartEvent>| {
                let starts = Arc::clone(&starts);
                let calls = model.calls();
                async move {
                    tokio::task::yield_now().await;
                    lock(&starts).push((event, calls));
                }
            }
        })
        .on_end({
            let ends = Arc::clone(&ends);
            let model = Arc::clone(&model);
            move |event: Arc<EmbedCallEndEvent>| {
                let ends = Arc::clone(&ends);
                let calls = model.calls();
                async move {
                    tokio::task::yield_now().await;
                    lock(&ends).push((event, calls));
                }
            }
        })
        .await
        .unwrap();
    let starts = lock(&starts);
    let ends = lock(&ends);
    assert_eq!((starts.len(), ends.len()), (1, 1));
    let call_id = starts[0].0.call_id.clone();
    assert_eq!(
        starts[0],
        (
            Arc::new(EmbedCallStartEvent {
                runtime_context: Some(json!({"request": "r-1"})),
                call_id: call_id.clone(),
                operation_id: "ai.embedMany",
                model: ModelIdentity::new("mock", "embed-mock"),
                value: Some(EmbeddingInput::Many(vec!["a".to_owned(), "bb".to_owned()])),
                max_retries: 1,
                headers: Headers::new()
                    .with("x-request", "r-1")
                    .with_user_agent_suffix([concat!("ferrin/", env!("CARGO_PKG_VERSION"))]),
                provider_options: ProviderOptions::new(),
            }),
            Vec::new()
        )
    );
    assert_eq!(
        ends[0],
        (
            Arc::new(EmbedCallEndEvent {
                runtime_context: Some(json!({"request": "r-1"})),
                call_id,
                operation_id: "ai.embedMany",
                model: ModelIdentity::new("mock", "embed-mock"),
                value: Some(EmbeddingInput::Many(result.values.clone())),
                embedding: Some(EmbeddingOutput::Many(result.embeddings.clone())),
                usage: result.usage,
                warnings: result.warnings.clone(),
                provider_metadata: result.provider_metadata.clone(),
                response: EmbeddingResponse::Many(result.responses),
            }),
            vec![
                vec!["a".to_owned()],
                vec!["a".to_owned()],
                vec!["bb".to_owned()]
            ]
        )
    );
}

#[tokio::test]
async fn single_embedding_hooks_keep_single_value_shapes_and_default_context() {
    let ends = Arc::new(Mutex::new(Vec::new()));
    let result = embed(Arc::new(EmbedMock::new()), "hello")
        .on_start(|_: Arc<EmbedCallStartEvent>| async { panic!("isolated callback panic") })
        .on_end({
            let ends = Arc::clone(&ends);
            move |event: Arc<EmbedCallEndEvent>| {
                let ends = Arc::clone(&ends);
                async move {
                    lock(&ends).push(event);
                }
            }
        })
        .await
        .unwrap();
    let ends = lock(&ends);
    assert_eq!(ends.len(), 1);
    assert_eq!(
        ends[0].as_ref(),
        &EmbedCallEndEvent {
            runtime_context: Some(json!({})),
            call_id: ends[0].call_id.clone(),
            operation_id: "ai.embed",
            model: ModelIdentity::new("mock", "embed-mock"),
            value: Some(EmbeddingInput::Single(result.value.clone())),
            embedding: Some(EmbeddingOutput::Single(result.embedding.clone())),
            usage: result.usage,
            warnings: result.warnings.clone(),
            provider_metadata: result.provider_metadata.clone(),
            response: EmbeddingResponse::Single(Box::new(result.response)),
        }
    );
}

#[tokio::test]
async fn failed_embedding_operation_does_not_emit_end() {
    let mut model = EmbedMock::new();
    model.drop_first = 1;
    let events = Arc::new(Mutex::new(Vec::new()));
    let result = embed(Arc::new(model), "hello")
        .on_start({
            let events = Arc::clone(&events);
            move |_: Arc<EmbedCallStartEvent>| {
                let events = Arc::clone(&events);
                async move {
                    lock(&events).push("start");
                }
            }
        })
        .on_end({
            let events = Arc::clone(&events);
            move |_: Arc<EmbedCallEndEvent>| {
                let events = Arc::clone(&events);
                async move {
                    lock(&events).push("end");
                }
            }
        })
        .await;
    assert!(result.is_err());
    assert_eq!(*lock(&events), vec!["start"]);
}
