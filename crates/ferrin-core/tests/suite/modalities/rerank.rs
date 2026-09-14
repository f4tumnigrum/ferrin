use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Error;
use ferrin_core::ErrorKind;
use ferrin_core::rerank;
use ferrin_core::rerank::RerankDocument;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RerankingModel;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ProviderError;
use ferrin_spec::reranking_model::RankedDocument;
use ferrin_spec::reranking_model::RerankOptions;
use ferrin_spec::reranking_model::RerankResult;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::lock;

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
