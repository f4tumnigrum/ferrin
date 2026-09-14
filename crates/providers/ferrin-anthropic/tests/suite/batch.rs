//! Message Batches API.

use ferrin_spec::Batch;
use ferrin_spec::Headers;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ResponseFormat;
use ferrin_spec::batch::BatchItem;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::batch::BatchListOptions;
use ferrin_spec::batch::BatchOperationOptions;
use ferrin_spec::batch::BatchRequest;
use ferrin_spec::batch::BatchStartOptions;
use ferrin_spec::batch::BatchState;
use ferrin_spec::batch::TextBatchRequestOptions;
use ferrin_spec::error::ProviderError;
use ferrin_testing::Fixture;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::common::TestProvider;
use super::common::anthropic_options;
use super::common::fixture_bytes;

fn text_request(id: &str, model: &str, text: &str) -> BatchRequest {
    BatchRequest::Text {
        id: id.to_owned(),
        model_id: model.into(),
        options: TextBatchRequestOptions {
            prompt: vec![PromptMessage::user_text(text)],
            ..TextBatchRequestOptions::default()
        },
    }
}

fn start_options(requests: Vec<BatchRequest>) -> BatchStartOptions {
    BatchStartOptions {
        requests,
        webhook_url: None,
        provider_options: ProviderOptions::new(),
        headers: Headers::new(),
        cancellation: CancellationToken::new(),
    }
}

fn ended_status(results_url: Option<&str>, archived: bool) -> serde_json::Value {
    json!({
        "id": "msgbatch_abc",
        "type": "message_batch",
        "processing_status": "ended",
        "request_counts": {"processing": 0, "succeeded": 1, "errored": 1, "canceled": 1, "expired": 2},
        "created_at": "2026-09-13T10:00:00Z",
        "expires_at": "2026-09-14T10:00:00Z",
        "archived_at": if archived { json!("2026-10-13T10:00:00Z") } else { json!(null) },
        "cancel_initiated_at": null,
        "ended_at": "2026-09-13T11:00:00Z",
        "results_url": results_url
    })
}

#[tokio::test]
async fn start_posts_every_request_with_merged_betas() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages/batches", "batch", "create");
    let batch = test.provider.batch();
    assert!(batch.supports_cancel_batch());
    assert!(batch.supports_list_batches());
    let mut options = start_options(vec![
        text_request("req-1", "claude-sonnet-4-5", "one"),
        text_request("req_2", "claude-opus-4-6", "two"),
    ]);
    options.webhook_url = Some(url::Url::parse("https://example.test/hook").unwrap());
    options.provider_options = anthropic_options(json!({"anthropicBeta": ["batch-beta"]}));
    let result = batch.do_start_batch(options).await.unwrap();
    assert_eq!(result.batch_id.as_str(), "msgbatch_abc");
    assert_eq!(result.status.status, BatchState::Pending);
    assert_eq!(result.status.raw_status.as_deref(), Some("in_progress"));
    let counts = result.status.request_counts.unwrap();
    assert_eq!(
        (
            counts.total,
            counts.pending,
            counts.completed,
            counts.failed
        ),
        (2, 2, 0, 0)
    );
    assert!(result.status.created_at.is_some());
    let metadata = result.status.provider_metadata.unwrap();
    assert_eq!(
        metadata["anthropic"]["requestCounts"]["processing"],
        json!(2)
    );
    assert_eq!(metadata["anthropic"]["resultsUrl"], json!(null));
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].request_id, None);

    let request = test.only_request();
    assert_eq!(request.header("anthropic-beta"), Some("batch-beta"));
    let body = request.body_json().unwrap();
    assert_eq!(body["requests"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["requests"][0]["custom_id"], json!("req-1"));
    assert_eq!(
        body["requests"][0]["params"]["model"],
        json!("claude-sonnet-4-5")
    );
    assert_eq!(body["requests"][0]["params"]["max_tokens"], json!(64000));
    assert_eq!(
        body["requests"][0]["params"]["messages"][0]["content"][0]["text"],
        json!("one")
    );
    assert_eq!(body["requests"][1]["custom_id"], json!("req_2"));
    assert!(body["requests"][1]["params"].get("stream").is_none());
}

#[tokio::test]
async fn invalid_requests_are_rejected_before_any_call() {
    let test = TestProvider::start().await;
    let batch = test.provider.batch();
    let invalid_argument = |error: ProviderError| {
        assert!(
            matches!(error, ProviderError::InvalidArgument(_)),
            "{error:?}"
        );
    };
    let unsupported = |error: ProviderError| {
        assert!(
            matches!(error, ProviderError::UnsupportedFunctionality(_)),
            "{error:?}"
        );
    };
    invalid_argument(
        batch
            .do_start_batch(start_options(vec![text_request(
                "bad id!",
                "claude-sonnet-4-5",
                "x",
            )]))
            .await
            .unwrap_err(),
    );
    invalid_argument(
        batch
            .do_start_batch(start_options(vec![
                text_request("dup", "claude-sonnet-4-5", "x"),
                text_request("dup", "claude-sonnet-4-5", "y"),
            ]))
            .await
            .unwrap_err(),
    );
    unsupported(
        batch
            .do_start_batch(start_options(vec![BatchRequest::Image {
                id: "img".to_owned(),
                model_id: "x".into(),
                options: ferrin_spec::batch::ImageBatchRequestOptions::default(),
            }]))
            .await
            .unwrap_err(),
    );

    let mut per_request_beta = text_request("beta", "claude-sonnet-4-5", "x");
    if let BatchRequest::Text { options, .. } = &mut per_request_beta {
        options.provider_options = anthropic_options(json!({"anthropicBeta": ["x"]}));
    }
    unsupported(
        batch
            .do_start_batch(start_options(vec![per_request_beta]))
            .await
            .unwrap_err(),
    );

    let mut fast = text_request("fast", "claude-sonnet-4-5", "x");
    if let BatchRequest::Text { options, .. } = &mut fast {
        options.provider_options = anthropic_options(json!({"speed": "fast"}));
    }
    unsupported(
        batch
            .do_start_batch(start_options(vec![fast]))
            .await
            .unwrap_err(),
    );

    let mut json_tool = text_request("json", "claude-3-haiku-20240307", "x");
    if let BatchRequest::Text { options, .. } = &mut json_tool {
        options.response_format = Some(ResponseFormat::Json {
            schema: Some(json!({"type": "object"})),
            name: None,
            description: None,
        });
    }
    unsupported(
        batch
            .do_start_batch(start_options(vec![json_tool]))
            .await
            .unwrap_err(),
    );
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn status_maps_counts_and_timestamps() {
    let test = TestProvider::start().await;
    test.mount(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc",
        "batch",
        "status-in-progress",
    );
    let status = test
        .provider
        .batch()
        .do_get_batch_status(BatchOperationOptions::new("msgbatch_abc"))
        .await
        .unwrap();
    assert_eq!(status.status, BatchState::Pending);
    let counts = status.request_counts.unwrap();
    assert_eq!(
        (
            counts.total,
            counts.pending,
            counts.completed,
            counts.failed
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(
        status.created_at.map(|time| time.to_rfc3339()),
        Some("2026-09-13T10:00:00+00:00".to_owned())
    );
    assert!(status.error.is_none());
}

#[tokio::test]
async fn list_passes_limit_and_cursor() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, "/v1/messages/batches", "batch", "list");
    let result = test
        .provider
        .batch()
        .do_list_batches(BatchListOptions {
            limit: Some(2),
            cursor: Some("msgbatch_000".to_owned()),
            ..BatchListOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(result.batches.len(), 2);
    assert_eq!(result.batches[0].batch_id.as_str(), "msgbatch_abc");
    assert_eq!(result.batches[0].status.status, BatchState::Completed);
    let counts = result.batches[0].status.request_counts.unwrap();
    assert_eq!((counts.total, counts.completed, counts.failed), (5, 3, 2));
    assert_eq!(result.batches[1].status.status, BatchState::Pending);
    assert_eq!(result.next_cursor.as_deref(), Some("msgbatch_def"));
    let request = test.only_request();
    assert_eq!(
        request.query.as_deref(),
        Some("limit=2&after_id=msgbatch_000")
    );
}

#[tokio::test]
async fn cancel_posts_an_empty_body() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages/batches/msgbatch_abc/cancel",
        "batch",
        "cancel",
    );
    test.provider
        .batch()
        .do_cancel_batch(BatchOperationOptions::new("msgbatch_abc"))
        .await
        .unwrap();
    assert_eq!(test.only_request().body_json().unwrap(), json!({}));
}

#[tokio::test]
async fn results_stream_maps_every_result_type() {
    let test = TestProvider::start().await;
    let results_url = test
        .server
        .url()
        .join("v1/messages/batches/msgbatch_abc/results")
        .unwrap();
    test.mount_fixture(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc",
        Fixture::json(&ended_status(Some(results_url.as_str()), false)),
    );
    test.mount_fixture(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc/results",
        Fixture::complete(
            StatusCode::OK,
            "application/x-jsonl",
            fixture_bytes("batch", "results.jsonl"),
        ),
    );
    let stream = test
        .provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("msgbatch_abc"))
        .await
        .unwrap();
    let items: Vec<BatchItemResult> = stream.map(|item| item.unwrap()).collect().await;
    assert_eq!(items.len(), 5);
    let BatchItemResult::Text(first) = &items[0] else {
        panic!("expected text item");
    };
    let BatchItem::Succeeded { id, result } = first.as_ref() else {
        panic!("expected success, got {first:?}");
    };
    assert_eq!(id, "req-1");
    assert_eq!(result.content[0].as_text(), Some("one"));
    assert_eq!(result.usage.input.total, Some(5));
    assert_eq!(result.response.id.as_deref(), Some("msg_batch_1"));
    let BatchItemResult::Text(second) = &items[1] else {
        panic!("expected text item");
    };
    let BatchItem::Failed {
        error,
        provider_metadata,
        ..
    } = second.as_ref()
    else {
        panic!("expected failure, got {second:?}");
    };
    assert_eq!(error.message, "prompt is too long");
    assert_eq!(error.error_type.as_deref(), Some("invalid_request_error"));
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["requestId"],
        json!("req_batch_2")
    );
    assert!(
        matches!(items[2], BatchItemResult::Text(ref item) if matches!(item.as_ref(), BatchItem::Cancelled { .. }))
    );
    assert!(
        matches!(items[3], BatchItemResult::Text(ref item) if matches!(item.as_ref(), BatchItem::Expired { .. }))
    );
    let BatchItemResult::Text(fifth) = &items[4] else {
        panic!("expected text item");
    };
    let BatchItem::Failed { error, .. } = fifth.as_ref() else {
        panic!("expected failure, got {fifth:?}");
    };
    assert_eq!(error.code.as_deref(), Some("invalid_response"));

    let received = test.server.received();
    assert_eq!(received.len(), 2);
    assert_eq!(received[1].header("x-api-key"), Some("test-key"));
}

async fn results_error(test: &TestProvider) -> ProviderError {
    match test
        .provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("msgbatch_abc"))
        .await
    {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

#[tokio::test]
async fn results_of_pending_or_archived_batches_are_rejected() {
    let test = TestProvider::start().await;
    test.mount(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc",
        "batch",
        "status-in-progress",
    );
    let error = results_error(&test).await;
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );

    let archived = TestProvider::start().await;
    archived.mount_fixture(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc",
        Fixture::json(&ended_status(Some("https://example.test/results"), true)),
    );
    let error = results_error(&archived).await;
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );

    let missing = TestProvider::start().await;
    missing.mount_fixture(
        Method::GET,
        "/v1/messages/batches/msgbatch_abc",
        Fixture::json(&ended_status(None, false)),
    );
    let error = results_error(&missing).await;
    assert!(
        matches!(error, ProviderError::InvalidResponseData(_)),
        "{error:?}"
    );
}
