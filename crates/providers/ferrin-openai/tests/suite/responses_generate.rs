//! Non-streaming Responses API calls replayed from fixtures.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
    ]
}

#[tokio::test]
async fn text_response_maps_content_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "text-basic");
    let result = test
        .provider
        .responses("gpt-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].as_text(), Some("Hello from Ferrin."));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.usage.input.total, Some(42));
    assert_eq!(result.usage.input.cache_read, Some(10));
    assert_eq!(result.usage.output.total, Some(12));
    assert_eq!(result.usage.output.reasoning, Some(4));
    assert_eq!(result.response.id.as_deref(), Some("resp_text_basic"));
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("gpt-5")
    );
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["responseId"], json!("resp_text_basic"));
    assert_eq!(metadata["openai"]["serviceTier"], json!("default"));
    assert!(result.warnings.is_empty());

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["model"], json!("gpt-5"));
    assert_eq!(request["input"][0]["role"], json!("developer"));
    assert_eq!(request["input"][1]["role"], json!("user"));
    assert_eq!(
        request["input"][1]["content"][0]["type"],
        json!("input_text")
    );
    assert!(request.get("stream").is_none());
}

#[tokio::test]
async fn tool_call_response_maps_to_tool_call_content() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "tool-call");
    let mut options = CallOptions::new(prompt());
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"], "additionalProperties": false}),
    )];
    let result = test
        .provider
        .responses("gpt-5")
        .do_generate(options)
        .await
        .unwrap();
    let call = result.content[0].as_tool_call().unwrap();
    assert_eq!(call.tool_call_id.as_str(), "call_1");
    assert_eq!(call.tool_name.as_str(), "get_weather");
    assert_eq!(call.input, "{\"city\":\"Paris\"}");
    assert!(!call.provider_executed);
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["tools"][0]["type"], json!("function"));
    assert_eq!(request["tools"][0]["name"], json!("get_weather"));
    assert_eq!(request["tools"][0].get("strict"), None);
}

#[tokio::test]
async fn reasoning_response_maps_summary_and_encrypted_content() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "reasoning");
    let result = test
        .provider
        .responses("o4-mini")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    let Content::Reasoning {
        text,
        provider_metadata,
    } = &result.content[0]
    else {
        panic!("expected reasoning, got {:?}", result.content[0]);
    };
    assert_eq!(text, "Thinking about the weather.");
    let metadata = provider_metadata.as_ref().unwrap();
    assert_eq!(metadata["openai"]["itemId"], json!("rs_1"));
    assert_eq!(
        metadata["openai"]["reasoningEncryptedContent"],
        json!("enc_1")
    );
    assert_eq!(result.content[1].as_text(), Some("It is sunny."));
}

#[tokio::test]
async fn api_error_maps_to_api_call_error_with_status_and_message() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "error-400");
    let error = test
        .provider
        .responses("gpt-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    let ProviderError::ApiCall(api) = &error else {
        panic!("expected api call error, got {error:?}");
    };
    assert_eq!(api.status_code, Some(StatusCode::BAD_REQUEST));
    assert!(
        api.message.contains("Unsupported parameter: 'foo'"),
        "{}",
        api.message
    );
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn rate_limit_error_is_retryable() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", "responses", "error-429");
    let error = test
        .provider
        .responses("gpt-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(error.is_retryable());
}
