//! Streaming Messages API calls replayed from fixtures.

use ferrin_anthropic::tools::AnthropicTools;
use ferrin_spec::CallOptions;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
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

fn text(parts: &[StreamPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn text_stream_emits_text_parts_and_finish() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages",
        "messages",
        "text-basic-stream",
    );
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_stream(options())
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    assert_eq!(text(&parts), "Hello streamed world.");
    assert!(matches!(
        &parts[1],
        StreamPart::ResponseMetadata { id: Some(id), .. } if id == "msg_stream"
    ));
    let (reason, usage) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::Stop);
    assert_eq!(usage.input.total, Some(52));
    assert_eq!(usage.input.cache_read, Some(10));
    assert_eq!(usage.output.total, Some(12));
    insta::assert_json_snapshot!("messages_stream_text", parts);

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream"], json!(true));
}

#[tokio::test]
async fn tool_call_stream_emits_input_deltas_then_tool_call() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "tool-call-stream");
    let mut options = options();
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        None,
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )];
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
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
    assert_eq!(call.tool_call_id.as_str(), "toolu_s1");
    assert_eq!(call.tool_name.as_str(), "get_weather");
    assert_eq!(call.input, "{\"city\": \"Paris\"}");
    let deltas: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolInputDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "{\"city\": \"Paris\"}");
    assert_eq!(finish(&parts).0.unified, FinishReasonKind::ToolCalls);
    insta::assert_json_snapshot!("messages_stream_tool_call", parts);

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["tools"][0]["eager_input_streaming"], json!(true));
}

#[tokio::test]
async fn reasoning_stream_emits_reasoning_with_signature_then_text() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "reasoning-stream");
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
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
    let signature = parts
        .iter()
        .find_map(|part| match part {
            StreamPart::ReasoningDelta {
                provider_metadata: Some(metadata),
                ..
            } => Some(metadata["anthropic"]["signature"].clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(signature, json!("sig_stream"));
    assert_eq!(text(&parts), "It is sunny.");
    assert_eq!(finish(&parts).1.output.reasoning, Some(25));
    insta::assert_json_snapshot!("messages_stream_reasoning", parts);
}

#[tokio::test]
async fn code_execution_stream_types_the_input_and_reports_the_container() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages",
        "messages",
        "code-execution-stream",
    );
    let mut options = options();
    options.tools = vec![
        AnthropicTools::new()
            .code_execution_20250825()
            .definition("code_execution".into(), None),
    ];
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_stream(options)
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    assert!(matches!(
        &parts[2],
        StreamPart::ToolInputStart { tool_name, provider_executed: true, .. }
            if tool_name.as_str() == "code_execution"
    ));
    let StreamPart::ToolInputDelta { delta, .. } = &parts[3] else {
        panic!("expected input delta, got {:?}", parts[3]);
    };
    assert!(
        delta.starts_with("{\"type\": \"programmatic-tool-call\","),
        "{delta}"
    );
    let call = parts
        .iter()
        .find_map(|part| match part {
            StreamPart::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    let input: serde_json::Value = serde_json::from_str(&call.input).unwrap();
    assert_eq!(
        input,
        json!({"type": "programmatic-tool-call", "code": "print(1)"})
    );
    let tool_result = parts
        .iter()
        .find_map(|part| match part {
            StreamPart::ToolResult(result) => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(tool_result.result["type"], json!("code_execution_result"));
    assert_eq!(tool_result.result["stdout"], json!("1\n"));
    let Some(StreamPart::Finish {
        provider_metadata: Some(metadata),
        ..
    }) = parts.last()
    else {
        panic!("expected finish with metadata");
    };
    assert_eq!(
        metadata["anthropic"]["container"]["id"],
        json!("container_1")
    );
    assert_eq!(
        metadata["anthropic"]["container"]["skills"][0]["skillId"],
        json!("pdf")
    );
    insta::assert_json_snapshot!("messages_stream_code_execution", parts);

    let request = test.only_request();
    assert_eq!(
        request.header("anthropic-beta"),
        Some("code-execution-2025-08-25")
    );
}

#[tokio::test]
async fn json_response_tool_stream_becomes_text() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "json-tool-stream");
    let mut options = options();
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({"type": "object", "properties": {"answer": {"type": "string"}}})),
        name: None,
        description: None,
    });
    let result = test
        .provider
        .messages("claude-3-haiku-20240307")
        .do_stream(options)
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    assert_eq!(text(&parts), "{\"answer\": \"42\"}");
    assert!(parts.iter().all(|part| !matches!(
        part,
        StreamPart::ToolCall(_) | StreamPart::ToolInputStart { .. }
    )));
    assert!(matches!(&parts[2], StreamPart::TextStart { id, .. } if id.as_str() == "0"));
    assert_eq!(finish(&parts).0.unified, FinishReasonKind::Stop);
}

#[tokio::test]
async fn error_before_output_fails_the_call() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages",
        "messages",
        "error-early-stream",
    );
    let error = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_stream(options())
        .await
        .unwrap_err();
    let ProviderError::ApiCall(api) = &error else {
        panic!("expected api call error, got {error:?}");
    };
    assert_eq!(api.message, "Overloaded");
    assert_eq!(api.status_code.map(|status| status.as_u16()), Some(529));
    assert!(error.is_retryable());
}

#[tokio::test]
async fn failure_after_output_closes_open_parts_and_ends_with_error() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages",
        "messages",
        "error-late-stream",
    );
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_stream(options())
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    let Some(StreamPart::Error { error }) = parts.last() else {
        panic!("expected a terminal error, got {:?}", parts.last());
    };
    assert_eq!(error.message, "Rate limit exceeded");
    assert_eq!(error.status_code, Some(429));
    assert_eq!(error.is_retryable, Some(true));
    assert!(matches!(
        &parts[parts.len() - 2],
        StreamPart::TextEnd { id, .. } if id.as_str() == "0"
    ));
    assert_eq!(text(&parts), "Partial");
}

#[tokio::test]
async fn raw_chunks_are_forwarded_when_requested() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/messages",
        "messages",
        "text-basic-stream",
    );
    let mut options = options();
    options.include_raw_chunks = true;
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_stream(options)
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    let raw = parts
        .iter()
        .filter(|part| matches!(part, StreamPart::Raw { .. }))
        .count();
    assert_eq!(raw, 8);
}
