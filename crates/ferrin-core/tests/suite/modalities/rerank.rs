use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Error;
use ferrin_core::ErrorKind;
use ferrin_core::rerank;
use ferrin_core::rerank::Ranked;
use ferrin_core::rerank::RerankCallEndEvent;
use ferrin_core::rerank::RerankCallStartEvent;
use ferrin_core::rerank::RerankDocument;
use ferrin_core::telemetry::ModelIdentity;
use ferrin_spec::Headers;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderOptions;
use ferrin_spec::RerankingModel;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ProviderError;
use ferrin_spec::reranking_model::RankedDocument;
use ferrin_spec::reranking_model::RerankOptions;
use ferrin_spec::reranking_model::RerankResult;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::lock;

#[derive(Default)]
struct OperationRecorder {
    starts: Mutex<Vec<RerankCallStartEvent>>,
    ends: Mutex<Vec<RerankCallEndEvent>>,
}

impl ferrin_core::telemetry::Telemetry for OperationRecorder {
    fn on_rerank_operation_start<'a>(
        &'a self,
        event: &'a RerankCallStartEvent,
    ) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.starts).push(event.clone());
        })
    }

    fn on_rerank_operation_end<'a>(
        &'a self,
        event: &'a RerankCallEndEvent,
    ) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.ends).push(event.clone());
        })
    }
}

#[tokio::test]
async fn operation_telemetry_hides_ranked_input_documents_when_only_outputs_are_recorded() {
    for documents in [Vec::new(), vec!["private document"]] {
        let application = Arc::new(OperationRecorder::default());
        let integration = Arc::new(OperationRecorder::default());
        let ranking = if documents.is_empty() {
            vec![]
        } else {
            vec![(0, 0.9)]
        };
        rerank(mock(ranking), "private query", documents)
            .runtime_context(json!({"private": true}))
            .telemetry(ferrin_core::TelemetryOptions {
                enabled: true,
                record_outputs: true,
                integrations: vec![integration.clone()],
                ..Default::default()
            })
            .on_start({
                let application = application.clone();
                move |event: Arc<RerankCallStartEvent>| {
                    let application = application.clone();
                    async move {
                        lock(&application.starts).push((*event).clone());
                    }
                }
            })
            .on_end({
                let application = application.clone();
                move |event: Arc<RerankCallEndEvent>| {
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
        assert_eq!(start.query, Some("private query".into()));
        assert!(end.ranking.is_some());
        start.runtime_context = None;
        start.documents = None;
        start.query = None;
        start.headers = Default::default();
        start.provider_options = Default::default();
        end.runtime_context = None;
        end.documents = None;
        end.query = None;
        end.ranking = None;
        assert_eq!(*lock(&integration.starts), vec![start]);
        assert_eq!(*lock(&integration.ends), vec![end]);
    }
}

struct RerankMock {
    provider: ProviderId,
    model_id: ModelId,
    ranking: Vec<(usize, f64)>,
    calls: Mutex<Vec<RerankOptions>>,
}

impl RerankingModel for RerankMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn do_rerank(
        &self,
        options: RerankOptions,
    ) -> impl Future<Output = Result<RerankResult, ProviderError>> + Send {
        lock(&self.calls).push(options);
        let ranking = self
            .ranking
            .iter()
            .map(|(index, score)| RankedDocument {
                index: *index,
                relevance_score: *score,
            })
            .collect();
        async move {
            Ok(RerankResult {
                ranking,
                provider_metadata: None,
                warnings: Vec::new(),
                response: ResponseMetadata::default(),
            })
        }
    }
}

fn mock(ranking: Vec<(usize, f64)>) -> Arc<RerankMock> {
    Arc::new(RerankMock {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("rerank-mock"),
        ranking,
        calls: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn reranks_text_documents() {
    let model = mock(vec![(2, 0.9), (0, 0.4)]);
    let result = rerank(Arc::clone(&model), "rust async", vec!["a", "b", "c"])
        .top_n(2)
        .await
        .unwrap();
    assert_eq!(result.original_documents, vec!["a", "b", "c"]);
    assert_eq!(result.ranking.len(), 2);
    assert_eq!(result.ranking[0].original_index, 2);
    assert_eq!(result.ranking[0].document, "c");
    assert_eq!(result.ranking[1].score, 0.4);
    assert_eq!(
        result.reranked_documents().copied().collect::<Vec<_>>(),
        vec!["c", "a"]
    );
    assert_eq!(
        result.response.model_id.as_ref().map(ModelId::as_str),
        Some("rerank-mock")
    );
    let calls = lock(&model.calls);
    assert_eq!(calls[0].top_n, Some(2));
    assert_eq!(calls[0].query, "rust async");
    assert_eq!(calls[0].documents.len(), 3);
}

#[tokio::test]
async fn reranks_object_documents() {
    let model = mock(vec![(1, 1.0)]);
    let docs = vec![
        json!({ "title": "one" }).as_object().cloned().unwrap(),
        json!({ "title": "two" }).as_object().cloned().unwrap(),
    ];
    let result = rerank(model, "q", docs).await.unwrap();
    assert_eq!(result.ranking[0].document["title"], "two");
}

#[tokio::test]
async fn mixed_documents_are_rejected() {
    let docs = vec![
        RerankDocument::Text("a".to_owned()),
        RerankDocument::Object(json!({ "x": 1 }).as_object().cloned().unwrap()),
    ];
    let error = rerank(mock(vec![]), "q", docs).await.unwrap_err();
    assert!(matches!(error, Error::InvalidArgument { .. }), "{error}");
}

#[tokio::test]
async fn empty_documents_skip_the_model() {
    let model = mock(vec![(0, 1.0)]);
    let result = rerank(Arc::clone(&model), "q", Vec::<String>::new())
        .await
        .unwrap();
    assert!(result.ranking.is_empty());
    assert!(lock(&model.calls).is_empty());
}

#[tokio::test]
async fn out_of_range_indices_are_provider_errors() {
    let error = rerank(mock(vec![(5, 1.0)]), "q", vec!["a"])
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Provider);
}

#[tokio::test]
async fn rerank_operation_hooks_receive_original_documents_and_ranked_result() {
    let model = mock(vec![(1, 0.9)]);
    let starts = Arc::new(Mutex::new(Vec::new()));
    let ends = Arc::new(Mutex::new(Vec::new()));
    let result = rerank(Arc::clone(&model), "query", vec!["a", "b"])
        .runtime_context(json!({"request": "r-2"}))
        .top_n(1)
        .header("x-request", "r-2")
        .on_start({
            let starts = Arc::clone(&starts);
            let model = Arc::clone(&model);
            move |event: Arc<RerankCallStartEvent>| {
                let starts = Arc::clone(&starts);
                let count = lock(&model.calls).len();
                async move {
                    tokio::task::yield_now().await;
                    lock(&starts).push((event, count));
                }
            }
        })
        .on_end({
            let ends = Arc::clone(&ends);
            let model = Arc::clone(&model);
            move |event: Arc<RerankCallEndEvent>| {
                let ends = Arc::clone(&ends);
                let count = lock(&model.calls).len();
                async move {
                    tokio::task::yield_now().await;
                    lock(&ends).push((event, count));
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
            Arc::new(RerankCallStartEvent {
                runtime_context: Some(json!({"request": "r-2"})),
                call_id: call_id.clone(),
                operation_id: "ai.rerank",
                model: ModelIdentity::new("mock", "rerank-mock"),
                documents: Some(vec![RerankDocument::from("a"), RerankDocument::from("b")]),
                query: Some("query".to_owned()),
                top_n: Some(1),
                max_retries: 2,
                headers: Headers::new().with("x-request", "r-2"),
                provider_options: ProviderOptions::new(),
            }),
            0
        )
    );
    assert_eq!(
        ends[0],
        (
            Arc::new(RerankCallEndEvent {
                runtime_context: Some(json!({"request": "r-2"})),
                call_id,
                operation_id: "ai.rerank",
                model: ModelIdentity::new("mock", "rerank-mock"),
                documents: Some(vec![RerankDocument::from("a"), RerankDocument::from("b")]),
                query: Some("query".to_owned()),
                ranking: Some(vec![Ranked {
                    original_index: 1,
                    score: 0.9,
                    document: RerankDocument::from("b"),
                }]),
                warnings: result.warnings.clone(),
                provider_metadata: result.provider_metadata.clone(),
                response: result.response,
            }),
            1
        )
    );
}

#[tokio::test]
async fn empty_rerank_emits_hooks_with_default_context_without_model_events() {
    use ferrin_core::Telemetry;
    use ferrin_core::TelemetryOptions;
    use ferrin_core::telemetry::RerankStartEvent;
    use ferrin_spec::BoxFuture;

    struct Recorder(Arc<Mutex<Vec<&'static str>>>);
    impl Telemetry for Recorder {
        fn on_rerank_start<'a>(&'a self, _: &'a RerankStartEvent) -> BoxFuture<'a, ()> {
            Box::pin(async {
                lock(&self.0).push("model");
            })
        }
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let result = rerank(mock(vec![]), "query", Vec::<String>::new())
        .telemetry(
            TelemetryOptions::enabled().with_integration(Arc::new(Recorder(Arc::clone(&events)))),
        )
        .on_start({
            let events = Arc::clone(&events);
            let contexts = Arc::clone(&contexts);
            move |event: Arc<RerankCallStartEvent>| {
                let events = Arc::clone(&events);
                let contexts = Arc::clone(&contexts);
                async move {
                    lock(&events).push("start");
                    lock(&contexts).push(event.runtime_context.clone());
                }
            }
        })
        .on_end({
            let events = Arc::clone(&events);
            let contexts = Arc::clone(&contexts);
            move |event: Arc<RerankCallEndEvent>| {
                let events = Arc::clone(&events);
                let contexts = Arc::clone(&contexts);
                async move {
                    lock(&events).push("end");
                    lock(&contexts).push(event.runtime_context.clone());
                }
            }
        })
        .await
        .unwrap();
    assert!(result.ranking.is_empty());
    assert_eq!(*lock(&events), vec!["start", "end"]);
    assert_eq!(*lock(&contexts), vec![Some(json!({})), Some(json!({}))]);
}

#[tokio::test]
async fn invalid_rerank_response_does_not_emit_operation_end() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let result = rerank(mock(vec![(5, 1.0)]), "query", vec!["a"])
        .on_start({
            let events = Arc::clone(&events);
            move |_: Arc<RerankCallStartEvent>| {
                let events = Arc::clone(&events);
                async move {
                    lock(&events).push("start");
                }
            }
        })
        .on_end({
            let events = Arc::clone(&events);
            move |_: Arc<RerankCallEndEvent>| {
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
