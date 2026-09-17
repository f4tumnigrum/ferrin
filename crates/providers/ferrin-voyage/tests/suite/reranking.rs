//! Wire request, result, validation and core integration coverage.

use ferrin_core::rerank;
use ferrin_core::rerank::Ranked;
use ferrin_spec::ModelId;
use ferrin_spec::Provider;
use ferrin_spec::RerankingModel;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::reranking_model::RankedDocument;
use ferrin_spec::reranking_model::RerankDocuments;
use ferrin_spec::reranking_model::RerankOptions;
use ferrin_spec::reranking_model::RerankResult;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::common::BASIC;
use crate::common::ERROR;
use crate::common::TestProvider;
use crate::common::provider_options;
use crate::common::text_options;

#[tokio::test]
async fn text_ranking_preserves_response_metadata_and_request_headers() {
    let test = TestProvider::start().await;
    let raw: serde_json::Value = serde_json::from_str(BASIC).unwrap();
    test.mount(raw.clone(), 200).await;
    let mut options = text_options();
    options.top_n = Some(2);
    options.provider_options = provider_options(
        "voyage",
        json!({"returnDocuments": true, "truncation": false}),
    );
    options.headers.insert("x-application", "call").unwrap();
    options.headers.insert("user-agent", "my-app/1").unwrap();
    let model = test.provider.reranking("rerank-requested");
    let result = model.do_rerank(options).await.unwrap();
    let headers = result.response.headers.clone().unwrap();
    assert_eq!(headers.get_str("x-request-id"), Some("fixture-rerank"));
    assert_eq!(
        result,
        RerankResult {
            ranking: vec![
                RankedDocument {
                    index: 1,
                    relevance_score: 0.9
                },
                RankedDocument {
                    index: 0,
                    relevance_score: 0.1
                }
            ],
            provider_metadata: None,
            warnings: vec![],
            response: ResponseMetadata {
                headers: Some(headers),
                body: Some(raw),
                model_id: Some(ModelId::new("rerank-2.5")),
                ..ResponseMetadata::default()
            },
        }
    );
    let requests = test.server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers["authorization"],
        "Bearer voyage-fixture-key"
    );
    assert_eq!(requests[0].headers["x-application"], "call");
    assert_eq!(
        requests[0].headers["user-agent"],
        concat!("my-app/1 ferrin-voyage/", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(test.transport.len(), 1);
    insta::assert_json_snapshot!(test.transport.last_request().unwrap().body_json().unwrap(), @r#"
    {
      "model": "rerank-requested",
      "query": "Rust async runtimes",
      "documents": [
        "A guide to gardening.",
        "Tokio is an async runtime."
      ],
      "top_k": 2,
      "return_documents": true,
      "truncation": false
    }
    "#);
}

#[tokio::test]
async fn object_documents_warn_and_core_preserves_original_objects() {
    let test = TestProvider::start().await;
    test.mount(json!({"data": [{"index": 1, "relevance_score": 0.9}]}), 200)
        .await;
    let documents = vec![
        json!({"title": "gardening"}).as_object().unwrap().clone(),
        json!({"title": "Tokio", "year": 2026})
            .as_object()
            .unwrap()
            .clone(),
    ];
    let model = Provider::reranking_model(&test.provider, "rerank-2.5").unwrap();
    let result = rerank(model, "Rust runtimes", documents.clone())
        .top_n(1)
        .await
        .unwrap();
    assert_eq!(
        result.ranking,
        vec![Ranked {
            original_index: 1,
            score: 0.9,
            document: documents[1].clone()
        }]
    );
    assert_eq!(
        result.warnings,
        vec![Warning::compatibility(
            "object documents",
            Some("object documents are converted to strings".into())
        )]
    );
    assert_eq!(result.response.model_id, Some(ModelId::new("rerank-2.5")));
    assert!(result.response.timestamp.is_some());
    insta::assert_json_snapshot!(test.transport.last_request().unwrap().body_json().unwrap(), @r#"
    {
      "model": "rerank-2.5",
      "query": "Rust runtimes",
      "documents": [
        "{\"title\":\"gardening\"}",
        "{\"title\":\"Tokio\",\"year\":2026}"
      ],
      "top_k": 1
    }
    "#);
}

#[tokio::test]
async fn rejects_invalid_rankings() {
    for ranking in [
        json!([{"index": 2, "relevance_score": 0.5}]),
        json!([{"index": 0, "relevance_score": 0.5}, {"index": 0, "relevance_score": 0.4}]),
        json!([{"index": 0, "relevance_score": 0.4}, {"index": 1, "relevance_score": 0.5}]),
    ] {
        let test = TestProvider::start().await;
        test.mount(json!({"data": ranking}), 200).await;
        let error = test
            .provider
            .reranking("rerank-2.5")
            .do_rerank(text_options())
            .await
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::InvalidResponseData(_)),
            "{error:?}"
        );
    }
    let test = TestProvider::start().await;
    test.mount(serde_json::from_str(BASIC).unwrap(), 200).await;
    let mut options = text_options();
    options.top_n = Some(1);
    let error = test
        .provider
        .reranking("rerank-2.5")
        .do_rerank(options)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidResponseData(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn rejects_malformed_entries_and_non_numeric_scores() {
    for body in [
        json!({}),
        json!({"data": [{"index": -1, "relevance_score": 0.5}]}),
        json!({"data": [{"index": 0.5, "relevance_score": 0.5}]}),
        json!({"data": [{"index": 0, "relevance_score": "NaN"}]}),
        json!({"data": [{"index": 0, "relevance_score": null}]}),
    ] {
        let test = TestProvider::start().await;
        test.mount(body, 200).await;
        let error = test
            .provider
            .reranking("rerank-2.5")
            .do_rerank(text_options())
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::ApiCall(_)), "{error:?}");
    }
}

#[tokio::test]
async fn failures_retain_detail_and_status_based_retryability() {
    for (status, retryable) in [(401, false), (429, true), (500, true)] {
        let test = TestProvider::start().await;
        test.mount(serde_json::from_str(ERROR).unwrap(), status)
            .await;
        let error = test
            .provider
            .reranking("rerank-2.5")
            .do_rerank(text_options())
            .await
            .unwrap_err();
        let ProviderError::ApiCall(error) = error else {
            panic!("{error:?}")
        };
        assert_eq!(
            (
                error.message.as_str(),
                error.status_code,
                error.is_retryable
            ),
            (
                "rate limit exceeded",
                Some(StatusCode::from_u16(status).unwrap()),
                retryable
            )
        );
    }
}

#[tokio::test]
async fn invalid_options_and_cancelled_calls_send_no_http_request() {
    let test = TestProvider::start().await;
    let model = test.provider.reranking("rerank-2.5");
    let mut invalid = text_options();
    invalid.top_n = Some(0);
    let empty = RerankOptions::new("q", RerankDocuments::Text { values: vec![] });
    let mut invalid_option = text_options();
    invalid_option.provider_options =
        provider_options("voyage", json!({"truncation": "secret-value"}));
    let mut unknown_option = text_options();
    unknown_option.provider_options = provider_options("voyage", json!({"unknown": true}));
    for options in [invalid, empty, invalid_option, unknown_option] {
        let error = model.do_rerank(options).await.unwrap_err();
        assert!(
            matches!(error, ProviderError::InvalidArgument(_)),
            "{error:?}"
        );
        assert!(!error.to_string().contains("secret-value"));
    }
    let cancelled = text_options();
    cancelled.cancellation.cancel();
    let error = model.do_rerank(cancelled).await.unwrap_err();
    assert!(matches!(error, ProviderError::Cancelled), "{error:?}");
    assert_eq!(test.server.received_requests().await.unwrap().len(), 0);
}
