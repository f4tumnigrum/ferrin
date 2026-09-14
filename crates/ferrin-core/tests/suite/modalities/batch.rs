use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Error;
use ferrin_core::StepContent;
use ferrin_core::batch::BatchItem;
use ferrin_core::batch::BatchRequest;
use ferrin_core::batch::BatchResultItem;
use ferrin_core::batch::ImageBatchRequest;
use ferrin_core::batch::TextBatchRequest;
use ferrin_core::cancel_batch;
use ferrin_core::get_batch_results;
use ferrin_core::get_batch_status;
use ferrin_core::list_batches;
use ferrin_core::start_batch;
use ferrin_spec::Batch;
use ferrin_spec::BatchId;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderId;
use ferrin_spec::SupportedUrls;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolDefinition;
use ferrin_spec::batch::BatchError;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::batch::BatchListItem;
use ferrin_spec::batch::BatchListOptions;
use ferrin_spec::batch::BatchListResult;
use ferrin_spec::batch::BatchOperationOptions;
use ferrin_spec::batch::BatchRequest as ModelBatchRequest;
use ferrin_spec::batch::BatchResultStream;
use ferrin_spec::batch::BatchStartOptions;
use ferrin_spec::batch::BatchStartResult;
use ferrin_spec::batch::BatchState;
use ferrin_spec::batch::BatchStatus;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;
use futures_util::stream;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::super::common::weather_tools;
use super::common::lock;

struct BatchMock {
    provider: ProviderId,
    starts: Mutex<Vec<BatchStartOptions>>,
    listing: bool,
}

impl Batch for BatchMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send {
        std::future::ready(SupportedUrls::none())
    }

    fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> impl Future<Output = Result<BatchStartResult, ProviderError>> + Send {
        let count = options.requests.len() as u64;
        lock(&self.starts).push(options);
        async move {
            let mut status = BatchStatus::new(BatchState::Pending);
            status.raw_status = Some(format!("queued:{count}"));
            Ok(BatchStartResult {
                batch_id: BatchId::new("batch-1"),
                status,
                warnings: Vec::new(),
            })
        }
    }

    async fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchStatus, ProviderError> {
        let mut status = BatchStatus::new(BatchState::Completed);
        status.raw_status = Some(options.batch_id.as_str().to_owned());
        Ok(status)
    }

    async fn do_get_batch_results(
        &self,
        _options: BatchOperationOptions,
    ) -> Result<BatchResultStream, ProviderError> {
        {
            let mut text = GenerateResult::new(
                vec![
                    Content::text("It is sunny."),
                    Content::ToolCall(ToolCall::new(
                        "call-1",
                        "get_weather",
                        json!({ "city": "Rome" }).to_string(),
                    )),
                ],
                FinishReason::tool_calls(),
            );
            text.response.id = Some("resp-1".to_owned());
            let items: Vec<Result<BatchItemResult, ProviderError>> = vec![
                Ok(BatchItemResult::Text(Box::new(BatchItem::Succeeded {
                    id: "r1".to_owned(),
                    result: text,
                }))),
                Ok(BatchItemResult::Text(Box::new(BatchItem::Failed {
                    id: "r2".to_owned(),
                    error: BatchError {
                        message: "rate limited".to_owned(),
                        error_type: None,
                        code: None,
                        status_code: Some(429),
                    },
                    provider_metadata: None,
                }))),
            ];
            let stream: BatchResultStream = Box::pin(stream::iter(items));
            Ok(stream)
        }
    }

    fn supports_list_batches(&self) -> bool {
        self.listing
    }

    async fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> Result<BatchListResult, ProviderError> {
        {
            Ok(BatchListResult {
                batches: vec![BatchListItem {
                    batch_id: BatchId::new("batch-1"),
                    status: BatchStatus::new(BatchState::Completed),
                }],
                next_cursor: options.limit.map(|limit| format!("after-{limit}")),
                provider_metadata: None,
            })
        }
    }
}

fn mock(listing: bool) -> Arc<BatchMock> {
    Arc::new(BatchMock {
        provider: ProviderId::new("mock"),
        starts: Mutex::new(Vec::new()),
        listing,
    })
}

#[tokio::test]
async fn starts_a_batch_with_converted_requests() {
    let api = mock(false);
    let requests: Vec<BatchRequest> = vec![
        TextBatchRequest::new("r1", "gpt-x")
            .system("Be brief.")
            .prompt("Weather in Rome?")
            .tools(weather_tools())
            .into(),
        ImageBatchRequest::new("r2", "img-x", "a cat").n(2).into(),
    ];
    let result = start_batch(Arc::clone(&api), requests).await.unwrap();
    assert_eq!(result.batch_id.as_str(), "batch-1");
    assert_eq!(result.status.raw_status.as_deref(), Some("queued:2"));
    let starts = lock(&api.starts);
    assert_eq!(starts.len(), 1);
    match &starts[0].requests[0] {
        ModelBatchRequest::Text {
            id,
            model_id,
            options,
        } => {
            assert_eq!(id, "r1");
            assert_eq!(model_id.as_str(), "gpt-x");
            assert_eq!(options.prompt.len(), 2);
            assert!(
                matches!(&options.prompt[0], PromptMessage::System { content, .. } if content == "Be brief.")
            );
            assert_eq!(options.tools.len(), 1);
            assert!(
                matches!(&options.tools[0], ToolDefinition::Function { name, .. } if name.as_str() == "get_weather")
            );
        }
        other => panic!("unexpected request: {other:?}"),
    }
    match &starts[0].requests[1] {
        ModelBatchRequest::Image { id, options, .. } => {
            assert_eq!(id, "r2");
            assert_eq!(options.n, 2);
            assert_eq!(options.prompt.as_deref(), Some("a cat"));
        }
        other => panic!("unexpected request: {other:?}"),
    }
    assert!(
        starts[0]
            .headers
            .get_str("user-agent")
            .is_some_and(|ua| ua.starts_with("ferrin/"))
    );
}

#[tokio::test]
async fn rejects_empty_and_duplicate_ids() {
    let error = start_batch(mock(false), Vec::<BatchRequest>::new())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidArgument { .. }), "{error}");
    let requests: Vec<BatchRequest> = vec![
        TextBatchRequest::new("dup", "m").prompt("a").into(),
        TextBatchRequest::new("dup", "m").prompt("b").into(),
    ];
    let error = start_batch(mock(false), requests).await.unwrap_err();
    assert!(error.to_string().contains("duplicate id `dup`"), "{error}");
}

#[tokio::test]
async fn status_results_cancel_and_list() {
    let api = mock(true);
    let status = get_batch_status(Arc::clone(&api), "batch-1").await.unwrap();
    assert_eq!(status.status, BatchState::Completed);
    assert_eq!(status.raw_status.as_deref(), Some("batch-1"));

    let items: Vec<BatchResultItem> = get_batch_results(Arc::clone(&api), "batch-1")
        .tools(weather_tools())
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id(), "r1");
    match &items[0] {
        BatchResultItem::Text(item) => match item.as_ref() {
            BatchItem::Succeeded { result, .. } => {
                assert_eq!(result.text(), "It is sunny.");
                assert_eq!(result.content.len(), 2);
                assert!(matches!(&result.content[1], StepContent::ToolCall(call) if !call.invalid));
                assert_eq!(result.response.id.as_deref(), Some("resp-1"));
            }
            other => panic!("unexpected item: {other:?}"),
        },
        other => panic!("unexpected item: {other:?}"),
    }
    match &items[1] {
        BatchResultItem::Text(item) => match item.as_ref() {
            BatchItem::Failed { error, .. } => assert_eq!(error.status_code, Some(429)),
            other => panic!("unexpected item: {other:?}"),
        },
        other => panic!("unexpected item: {other:?}"),
    }

    let error = cancel_batch(Arc::clone(&api), "batch-1").await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");

    let list = list_batches(Arc::clone(&api)).limit(10).await.unwrap();
    assert_eq!(list.batches.len(), 1);
    assert_eq!(list.next_cursor.as_deref(), Some("after-10"));

    let error = list_batches(mock(false)).await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");
}
