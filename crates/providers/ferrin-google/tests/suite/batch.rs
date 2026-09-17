//! Batch generation: start, status, results, cancel and list.

use ferrin_spec::Batch;
use ferrin_spec::FileData;
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
use ferrin_spec::batch::ImageBatchRequestOptions;
use ferrin_spec::batch::TextBatchRequestOptions;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageFile;
use ferrin_testing::Fixture;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::common::TestProvider;
use super::common::fixture_bytes;
use super::common::google_options;

const BATCH_PATH: &str = "/v1beta/batches/batch-123";

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

async fn collect(test: &TestProvider) -> Vec<BatchItemResult> {
    let stream = test
        .provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("batches/batch-123"))
        .await
        .unwrap();
    stream.map(|item| item.unwrap()).collect::<Vec<_>>().await
}

fn expect_err<T>(result: Result<T, ProviderError>) -> ProviderError {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

fn text_item(item: &BatchItemResult) -> &BatchItem<ferrin_spec::GenerateResult> {
    match item {
        BatchItemResult::Text(item) => item,
        other => panic!("expected text item, got {other:?}"),
    }
}

#[tokio::test]
async fn start_posts_inline_requests_with_per_request_warnings() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/gemini-2.5-flash:batchGenerateContent",
        "batch",
        "create",
    );
    let batch = test.provider.batch();
    assert!(batch.supports_cancel_batch());
    assert!(batch.supports_list_batches());
    let mut second = text_request("req-2", "gemini-2.5-flash", "two");
    if let BatchRequest::Text { options, .. } = &mut second {
        options.presence_penalty = Some(0.5);
        options.max_output_tokens = Some(50);
        options.provider_options = google_options(json!({"cachedContent": "cachedContents/a"}));
    }
    let mut options = start_options(vec![
        text_request("req-1", "gemini-2.5-flash", "one"),
        second,
    ]);
    options.webhook_url = Some(url::Url::parse("https://example.test/hook").unwrap());
    let result = batch.do_start_batch(options).await.unwrap();
    assert_eq!(result.batch_id.as_str(), "batches/batch-123");
    assert_eq!(result.status.status, BatchState::Pending);
    assert_eq!(
        result.status.raw_status.as_deref(),
        Some("BATCH_STATE_PENDING")
    );
    assert_eq!(
        result.status.request_counts,
        Some(ferrin_spec::batch::BatchRequestCounts {
            total: 2,
            pending: 2,
            completed: 0,
            failed: 0,
        })
    );
    assert!(result.status.created_at.is_some());
    assert!(result.status.provider_metadata.is_none());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].request_id.as_deref(), Some("req-2"));
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("batch_start_request", request);
}

#[tokio::test]
async fn image_requests_are_converted_like_the_image_model() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/gemini-2.5-flash-image:batchGenerateContent",
        "batch",
        "create",
    );
    let request = BatchRequest::Image {
        id: "img-1".to_owned(),
        model_id: "gemini-2.5-flash-image".into(),
        options: ImageBatchRequestOptions {
            prompt: Some("A red square".to_owned()),
            n: 1,
            aspect_ratio: Some(ferrin_spec::AspectRatio::new(1, 1)),
            files: vec![ImageFile {
                data: FileData::Bytes {
                    data: bytes::Bytes::from_static(b"\x89PNG"),
                },
                media_type: Some("image/png".into()),
                provider_options: None,
            }],
            ..ImageBatchRequestOptions::default()
        },
    };
    let result = test
        .provider
        .batch()
        .do_start_batch(start_options(vec![request]))
        .await
        .unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let body = test.only_request().body_json().unwrap();
    let inner = &body["batch"]["inputConfig"]["requests"]["requests"][0]["request"];
    assert_eq!(
        inner["generationConfig"]["responseModalities"],
        json!(["IMAGE"])
    );
    assert_eq!(
        inner["generationConfig"]["imageConfig"],
        json!({"aspectRatio": "1:1"})
    );
    assert_eq!(inner["contents"][0]["parts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn start_rejects_empty_and_mixed_model_batches() {
    let test = TestProvider::start().await;
    let batch = test.provider.batch();
    let error = batch
        .do_start_batch(start_options(Vec::new()))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    let error = batch
        .do_start_batch(start_options(vec![
            text_request("a", "gemini-2.5-flash", "one"),
            text_request("b", "gemini-2.5-pro", "two"),
        ]))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn status_maps_states_counts_and_errors() {
    let test = TestProvider::start().await;
    let batch = test.provider.batch();
    test.mount(Method::GET, BATCH_PATH, "batch", "status-running");
    let status = batch
        .do_get_batch_status(BatchOperationOptions::new("batches/batch-123"))
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

    test.server.reset();
    test.mount(Method::GET, BATCH_PATH, "batch", "status-failed");
    let status = batch
        .do_get_batch_status(BatchOperationOptions::new("batches/batch-123"))
        .await
        .unwrap();
    assert_eq!(status.status, BatchState::Failed);
    let error = status.error.unwrap();
    assert_eq!(error.message, "internal error while running the batch");
    assert_eq!(error.error_type.as_deref(), Some("INTERNAL"));
    assert_eq!(error.code.as_deref(), Some("13"));

    test.server.reset();
    test.mount(Method::GET, BATCH_PATH, "batch", "status-succeeded-inline");
    let status = batch
        .do_get_batch_status(BatchOperationOptions::new("batches/batch-123"))
        .await
        .unwrap();
    assert_eq!(status.status, BatchState::Completed);
    assert_eq!(status.request_counts.unwrap().completed, 2);
}

#[tokio::test]
async fn inlined_results_map_to_items() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, BATCH_PATH, "batch", "status-succeeded-inline");
    let items = collect(&test).await;
    assert_eq!(items.len(), 4);
    let BatchItem::Succeeded { id, result } = text_item(&items[0]) else {
        panic!("expected success, got {:?}", items[0]);
    };
    assert_eq!(id, "req-1");
    assert_eq!(result.content[0].as_text(), Some("one"));
    assert_eq!(result.usage.input.total, Some(3));
    assert_eq!(result.response.id.as_deref(), Some("resp-batch-1"));
    let BatchItem::Cancelled { id, error, .. } = text_item(&items[1]) else {
        panic!("expected cancelled, got {:?}", items[1]);
    };
    assert_eq!(id, "req-2");
    assert_eq!(error.as_ref().unwrap().code.as_deref(), Some("1"));
    let BatchItem::Failed {
        error,
        provider_metadata,
        ..
    } = text_item(&items[2])
    else {
        panic!("expected failure, got {:?}", items[2]);
    };
    assert_eq!(error.code.as_deref(), Some("prompt_blocked"));
    assert_eq!(error.error_type.as_deref(), Some("SAFETY"));
    assert_eq!(
        provider_metadata.as_ref().unwrap()["google"]["promptFeedback"]["blockReason"],
        json!("SAFETY")
    );
    let BatchItemResult::Image(image) = &items[3] else {
        panic!("expected image item, got {:?}", items[3]);
    };
    let BatchItem::Succeeded { id, result } = image.as_ref() else {
        panic!("expected image success");
    };
    assert_eq!(id, "req-4");
    assert_eq!(result.images.len(), 1);
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("gemini-2.5-flash-image")
    );
}

#[tokio::test]
async fn file_results_are_downloaded_as_json_lines() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, BATCH_PATH, "batch", "status-succeeded-file");
    test.mount_fixture(
        Method::GET,
        "/download/v1beta/files/batch-123-output:download",
        Fixture::complete(
            StatusCode::OK,
            "application/jsonl",
            fixture_bytes("batch", "results.jsonl"),
        ),
    );
    let items = collect(&test).await;
    let codes: Vec<(String, Option<String>)> = items
        .iter()
        .map(|item| match text_item(item) {
            BatchItem::Succeeded { id, .. } => (id.clone(), None),
            BatchItem::Failed { id, error, .. } => (id.clone(), error.code.clone()),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            ("req-1".to_owned(), None),
            ("req-2".to_owned(), Some("3".to_owned())),
            ("req-3".to_owned(), Some("invalid_batch_result".to_owned())),
            ("req-4".to_owned(), Some("unsupported_content".to_owned())),
            ("req-5".to_owned(), Some("invalid_response".to_owned())),
        ]
    );
    let download = test.server.received().into_iter().nth(1).unwrap();
    assert_eq!(download.query.as_deref(), Some("alt=media"));
    assert_eq!(download.header("x-goog-api-key"), Some("test-key"));
}

#[tokio::test]
async fn results_require_a_finished_batch_with_output() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, BATCH_PATH, "batch", "status-running");
    let error = expect_err(
        test.provider
            .batch()
            .do_get_batch_results(BatchOperationOptions::new("batches/batch-123"))
            .await,
    );
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    test.server.reset();
    test.mount_fixture(
        Method::GET,
        BATCH_PATH,
        Fixture::json(&json!({
            "name": "batches/batch-123",
            "done": true,
            "metadata": {"state": "BATCH_STATE_SUCCEEDED"}
        })),
    );
    let error = expect_err(
        test.provider
            .batch()
            .do_get_batch_results(BatchOperationOptions::new("batches/batch-123"))
            .await,
    );
    assert!(
        matches!(error, ProviderError::InvalidResponseData(_)),
        "{error:?}"
    );
    test.server.reset();
    test.mount(Method::GET, BATCH_PATH, "batch", "status-failed");
    let items = collect(&test).await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn cancel_and_list_use_the_operation_endpoints() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/batches/batch-123:cancel",
        "batch",
        "cancel",
    );
    test.mount(Method::GET, "/v1beta/batches", "batch", "list");
    let batch = test.provider.batch();
    batch
        .do_cancel_batch(BatchOperationOptions::new("batches/batch-123"))
        .await
        .unwrap();
    let list = batch
        .do_list_batches(BatchListOptions {
            limit: Some(2),
            cursor: Some("page-1".to_owned()),
            ..BatchListOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(list.next_cursor.as_deref(), Some("page-2"));
    let states: Vec<(&str, BatchState)> = list
        .batches
        .iter()
        .map(|item| (item.batch_id.as_str(), item.status.status))
        .collect();
    assert_eq!(
        states,
        vec![
            ("batches/batch-123", BatchState::Completed),
            ("batches/batch-456", BatchState::Pending),
            ("batches/batch-789", BatchState::Completed),
        ]
    );
    let requests = test.server.received();
    assert_eq!(requests[0].body_json().unwrap(), json!({}));
    assert_eq!(
        requests[1].query.as_deref(),
        Some("pageSize=2&pageToken=page-1")
    );
}
