//! Batch API.

use bytes::Bytes;
use ferrin_spec::Batch;
use ferrin_spec::Headers;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderOptions;
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
use super::common::fixture_bytes;
use super::common::openai_options;

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

#[tokio::test]
async fn start_uploads_jsonl_then_creates_the_batch() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/files", "batch", "file-upload");
    test.mount(Method::POST, "/v1/batches", "batch", "create");
    let batch = test.provider.batch();
    assert!(batch.supports_cancel_batch());
    assert!(batch.supports_list_batches());
    let mut options = start_options(vec![
        text_request("req-1", "gpt-5", "one"),
        text_request("req-2", "gpt-5", "two"),
    ]);
    options.webhook_url = Some(url::Url::parse("https://example.test/hook").unwrap());
    options.provider_options = openai_options(json!({"inputFileExpiresAfter": 7200}));
    let result = batch.do_start_batch(options).await.unwrap();
    assert_eq!(result.batch_id.as_str(), "batch_abc");
    assert_eq!(result.status.status, BatchState::Pending);
    assert_eq!(result.status.raw_status.as_deref(), Some("validating"));
    let metadata = result.status.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["inputFileId"], json!("file-batch-in"));
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].request_id, None);

    let received = test.server.received();
    assert_eq!(received.len(), 2);
    let upload = received[0].body_text();
    assert!(upload.contains("name=\"purpose\"\r\n\r\nbatch"), "{upload}");
    assert!(
        upload.contains("name=\"expires_after[seconds]\"\r\n\r\n7200"),
        "{upload}"
    );
    assert!(upload.contains("filename=\"batch.jsonl\""), "{upload}");
    let line = upload
        .lines()
        .find(|line| line.starts_with("{\"custom_id\":\"req-1\""))
        .unwrap();
    let line: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(line["method"], json!("POST"));
    assert_eq!(line["url"], json!("/v1/responses"));
    assert_eq!(line["body"]["model"], json!("gpt-5"));
    assert_eq!(line["body"]["input"][0]["role"], json!("user"));
    let create = received[1].body_json().unwrap();
    assert_eq!(
        create,
        json!({"input_file_id": "file-batch-in", "endpoint": "/v1/responses", "completion_window": "24h"})
    );
}

#[tokio::test]
async fn mixed_models_and_image_requests_are_rejected() {
    let test = TestProvider::start().await;
    let batch = test.provider.batch();
    let error = batch
        .do_start_batch(start_options(vec![
            text_request("a", "gpt-5", "x"),
            text_request("b", "gpt-4.1", "y"),
        ]))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    let error = batch
        .do_start_batch(start_options(vec![BatchRequest::Image {
            id: "img".to_owned(),
            model_id: "gpt-image-1".into(),
            options: ferrin_spec::batch::ImageBatchRequestOptions::default(),
        }]))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn status_maps_counts_errors_and_timestamps() {
    let test = TestProvider::start().await;
    test.mount(
        Method::GET,
        "/v1/batches/batch_abc",
        "batch",
        "retrieve-failed",
    );
    let status = test
        .provider
        .batch()
        .do_get_batch_status(BatchOperationOptions::new("batch_abc"))
        .await
        .unwrap();
    assert_eq!(status.status, BatchState::Failed);
    assert_eq!(status.raw_status.as_deref(), Some("failed"));
    let counts = status.request_counts.unwrap();
    assert_eq!(
        (
            counts.total,
            counts.pending,
            counts.completed,
            counts.failed
        ),
        (2, 2, 0, 0)
    );
    let error = status.error.unwrap();
    assert_eq!(error.code.as_deref(), Some("invalid_json_line"));
    assert!(status.created_at.is_some());
    assert!(status.expires_at.is_some());
}

#[tokio::test]
async fn results_of_a_pending_batch_are_an_invalid_argument() {
    let test = TestProvider::start().await;
    test.mount(
        Method::GET,
        "/v1/batches/batch_abc",
        "batch",
        "retrieve-pending",
    );
    let Err(error) = test
        .provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("batch_abc"))
        .await
    else {
        panic!("expected an error for a pending batch");
    };
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn results_stream_output_and_error_files() {
    let test = TestProvider::start().await;
    test.mount(
        Method::GET,
        "/v1/batches/batch_abc",
        "batch",
        "retrieve-completed",
    );
    test.mount_fixture(
        Method::GET,
        "/v1/files/file-batch-out/content",
        Fixture::complete(
            StatusCode::OK,
            "application/jsonl",
            Bytes::from(fixture_bytes("batch", "output.jsonl")),
        ),
    );
    test.mount_fixture(
        Method::GET,
        "/v1/files/file-batch-err/content",
        Fixture::complete(
            StatusCode::OK,
            "application/jsonl",
            Bytes::from(fixture_bytes("batch", "errors.jsonl")),
        ),
    );
    let stream = test
        .provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("batch_abc"))
        .await
        .unwrap();
    let items: Vec<BatchItemResult> = stream.map(|item| item.unwrap()).collect().await;
    assert_eq!(items.len(), 3);
    let BatchItemResult::Text(first) = &items[0] else {
        panic!("expected text item");
    };
    let BatchItem::Succeeded { id, result } = first.as_ref() else {
        panic!("expected success, got {first:?}");
    };
    assert_eq!(id, "req-1");
    assert_eq!(result.content[0].as_text(), Some("Batch answer one."));
    assert_eq!(result.usage.input.total, Some(5));
    let BatchItemResult::Text(second) = &items[1] else {
        panic!("expected text item");
    };
    let BatchItem::Failed { id, error, .. } = second.as_ref() else {
        panic!("expected failure, got {second:?}");
    };
    assert_eq!(id, "req-2");
    assert_eq!(error.status_code, Some(400));
    assert_eq!(error.code.as_deref(), Some("model_not_found"));
    let BatchItemResult::Text(third) = &items[2] else {
        panic!("expected text item");
    };
    assert!(
        matches!(third.as_ref(), BatchItem::Expired { .. }),
        "{third:?}"
    );
}

#[tokio::test]
async fn cancel_and_list_hit_the_expected_endpoints() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/batches/batch_abc/cancel",
        "batch",
        "cancel",
    );
    test.mount(
        Method::GET,
        "/v1/batches?limit=2&after=batch_000",
        "batch",
        "list",
    );
    let batch = test.provider.batch();
    batch
        .do_cancel_batch(BatchOperationOptions::new("batch_abc"))
        .await
        .unwrap();
    let list = batch
        .do_list_batches(BatchListOptions {
            limit: Some(2),
            cursor: Some("batch_000".to_owned()),
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(list.batches.len(), 2);
    assert_eq!(list.batches[0].status.status, BatchState::Completed);
    assert_eq!(list.batches[1].batch_id.as_str(), "batch_def");
    assert_eq!(list.next_cursor.as_deref(), Some("batch_def"));
    let received = test.server.received();
    assert_eq!(received[0].body_json().unwrap(), json!({}));
    assert_eq!(
        received[1].query.as_deref(),
        Some("limit=2&after=batch_000")
    );
}
