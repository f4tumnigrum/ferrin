//! Chat Completions streaming.

use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::error::ProviderError;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::without_raw_usage;

fn prompt() -> Vec<PromptMessage> {
    vec![PromptMessage::user_text("Say hello")]
}

async fn stream_parts(test: &TestProvider, case: &str, include_raw: bool) -> Vec<StreamPart> {
    test.mount(Method::POST, "/v1/chat/completions", "chat", case);
    let mut options = CallOptions::new(prompt());
    options.include_raw_chunks = include_raw;
    let result = test
        .provider
        .chat("example-model")
        .do_stream(options)
        .await
        .unwrap();
    without_raw_usage(collect_checked(result).await)
}

#[tokio::test]
async fn stream_emits_text_and_finish() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "text-basic-stream", false).await;
    insta::assert_json_snapshot!("chat_stream_text", parts);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream"], json!(true));
    assert!(request.get("stream_options").is_none());
}

#[tokio::test]
async fn include_usage_adds_stream_options() {
    let test = TestProvider::start_with(|mut settings| {
        settings.include_usage = true;
        settings
    })
    .await;
    stream_parts(&test, "text-basic-stream", false).await;
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream_options"], json!({"include_usage": true}));
}

#[tokio::test]
async fn raw_chunks_are_forwarded_when_requested() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "text-basic-stream", true).await;
    let raw = parts
        .iter()
        .filter(|part| matches!(part, StreamPart::Raw { .. }))
        .count();
    assert_eq!(raw, 5);
}

#[tokio::test]
async fn stream_emits_reasoning_before_text() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "reasoning-stream", false).await;
    insta::assert_json_snapshot!("chat_stream_reasoning", parts);
}

#[tokio::test]
async fn stream_buffers_tool_call_deltas_until_the_name_arrives() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "tool-call-stream", false).await;
    let calls: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].tool_call_id.as_str(), "call_cs1");
    assert_eq!(calls[0].input, "{\"city\":\"Paris\"}");
    assert_eq!(calls[1].tool_call_id.as_str(), "call_cs2");
    assert_eq!(
        calls[1].provider_metadata.as_ref().unwrap()["example"]["thoughtSignature"],
        json!("sig-456")
    );
    insta::assert_json_snapshot!("chat_stream_tool_call", parts);
}

#[tokio::test]
async fn stream_without_a_finish_reason_ends_with_an_error() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "no-finish-stream", false).await;
    assert!(
        !parts
            .iter()
            .any(|part| matches!(part, StreamPart::Finish { .. }))
    );
    let Some(StreamPart::Error { error }) = parts.last() else {
        panic!("expected a terminal error, got {parts:#?}");
    };
    assert!(error.message.contains("finish reason"), "{}", error.message);
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, StreamPart::TextEnd { .. }))
    );
}

#[tokio::test]
async fn early_error_frame_fails_the_stream_call() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "error-early");
    let error = test
        .provider
        .chat("example-model")
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::ApiCall(_)), "{error:?}");
    assert_eq!(error.status_code(), Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.is_retryable());
    assert!(error.to_string().contains("overloaded"), "{error}");
}

#[tokio::test]
async fn late_error_frame_is_terminal() {
    let test = TestProvider::start().await;
    let parts = stream_parts(&test, "error-late", false).await;
    let deltas: Vec<&str> = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["Before "]);
    assert!(
        !parts
            .iter()
            .any(|part| matches!(part, StreamPart::Finish { .. }))
    );
    let Some(StreamPart::Error { error }) = parts.last() else {
        panic!("expected a terminal error, got {parts:#?}");
    };
    assert_eq!(error.message, "Rate limit reached");
    assert_eq!(error.status_code, Some(429));
    assert_eq!(error.is_retryable, Some(true));
}
