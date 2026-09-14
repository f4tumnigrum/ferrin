//! Streaming Responses API calls replayed from fixtures.

use ferrin_spec::CallOptions;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::without_raw_usage;

fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text("Say hello")])
}

fn finish(parts: &[StreamPart]) -> (&ferrin_spec::FinishReason, &ferrin_spec::Usage) {
    let Some(StreamPart::Finish {
        finish_reason,
        usage,
        ..
    }) = parts.last()
    else {
        panic!("expected finish, got {:?}", parts.last());
    };
    (finish_reason, usage)
}

#[tokio::test]
async fn text_stream_emits_text_parts_and_finish() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/responses",
        "responses",
        "text-basic-stream",
    );
    let result = test
        .provider
        .responses("gpt-5")
        .do_stream(options())
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello streamed world.");
    let (reason, usage) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::Stop);
    assert_eq!(usage.input.total, Some(42));
    assert_eq!(usage.output.total, Some(12));
    insta::assert_json_snapshot!("responses_stream_text", parts);

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream"], json!(true));
}

#[tokio::test]
async fn tool_call_stream_emits_input_deltas_then_tool_call() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/responses",
        "responses",
        "tool-call-stream",
    );
    let mut options = options();
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        None,
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )];
    let result = test
        .provider
        .responses("gpt-5")
        .do_stream(options)
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    let call = parts
        .iter()
        .find_map(|part| match part {
            StreamPart::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(call.tool_call_id.as_str(), "call_s1");
    assert_eq!(call.tool_name.as_str(), "get_weather");
    assert_eq!(call.input, "{\"city\":\"Paris\"}");
    let deltas: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolInputDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "{\"city\":\"Paris\"}");
    assert_eq!(finish(&parts).0.unified, FinishReasonKind::ToolCalls);
    insta::assert_json_snapshot!("responses_stream_tool_call", parts);
}

#[tokio::test]
async fn reasoning_stream_emits_reasoning_then_text() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/responses",
        "responses",
        "reasoning-stream",
    );
    let result = test
        .provider
        .responses("o4-mini")
        .do_stream(options())
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    let reasoning: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ReasoningDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasoning, "Considering the forecast.");
    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "It is sunny.");
    insta::assert_json_snapshot!("responses_stream_reasoning", parts);
}

#[tokio::test]
async fn error_before_output_fails_the_call() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "error-early");
    let error = test
        .provider
        .responses("gpt-5")
        .do_stream(options())
        .await
        .unwrap_err();
    let ProviderError::ApiCall(api) = &error else {
        panic!("expected api call error, got {error:?}");
    };
    assert!(
        api.message.contains("server had an error"),
        "{}",
        api.message
    );
}

#[tokio::test]
async fn failure_after_output_closes_open_parts_and_ends_with_error() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "error-late");
    let result = test
        .provider
        .responses("gpt-5")
        .do_stream(options())
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    let error = parts
        .iter()
        .find_map(|part| match part {
            StreamPart::Error { error } => Some(error),
            _ => None,
        })
        .unwrap();
    assert!(error.message.contains("Rate limit"), "{}", error.message);
    assert_eq!(error.status_code, Some(429));
    // The error is terminal: the open text part is closed first and no
    // finish part follows.
    assert!(matches!(parts.last(), Some(StreamPart::Error { .. })));
    assert!(matches!(
        &parts[parts.len() - 2],
        StreamPart::TextEnd { id, .. } if id.as_str() == "msg_s1"
    ));
}

#[tokio::test]
async fn raw_chunks_are_forwarded_when_requested() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/responses",
        "responses",
        "text-basic-stream",
    );
    let mut options = options();
    options.include_raw_chunks = true;
    let result = test
        .provider
        .responses("gpt-5")
        .do_stream(options)
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    let raw = parts
        .iter()
        .filter(|part| matches!(part, StreamPart::Raw { .. }))
        .count();
    assert_eq!(raw, 10);
}
