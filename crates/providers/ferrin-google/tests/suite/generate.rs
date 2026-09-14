//! Non-streaming `generateContent` calls replayed from fixtures.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FileData;
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
use super::common::google_options;
use super::common::options_under;

fn path(model: &str) -> String {
    format!("/v1beta/models/{model}:generateContent")
}

fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text(
        "How many r's are in strawberry?",
    )])
}

#[tokio::test]
async fn text_response_maps_content_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "generate",
        "text",
    );
    let result = test
        .provider
        .language_model("gemini-3-pro-preview")
        .do_generate(options())
        .await
        .unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(
        result.content[0].as_text(),
        Some("There are **3** r's in strawberry.\n\nHere is the breakdown: st**r**awbe**rr**y.")
    );
    let Content::Text {
        provider_metadata: Some(metadata),
        ..
    } = &result.content[0]
    else {
        panic!("expected text with metadata");
    };
    assert!(metadata["google"]["thoughtSignature"].is_string());
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.finish_reason.raw.as_deref(), Some("STOP"));
    assert_eq!(result.usage.input.total, Some(9));
    assert_eq!(result.usage.output.total, Some(272));
    assert_eq!(result.usage.output.text, Some(28));
    assert_eq!(result.usage.output.reasoning, Some(244));
    assert_eq!(
        result.response.id.as_deref(),
        Some("Un6LacrVMcjUxs0PmJfWoQc")
    );
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("gemini-3-pro-preview")
    );
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["google"]["usageMetadata"]["thoughtsTokenCount"],
        json!(244)
    );
    assert_eq!(metadata["google"]["promptFeedback"], json!(null));
    assert!(result.response.headers.is_some());
    let request = test.only_request();
    assert_eq!(request.path, path("gemini-3-pro-preview"));
    let body = request.body_json().unwrap();
    assert_eq!(
        body["contents"],
        json!([{"role": "user", "parts": [{"text": "How many r's are in strawberry?"}]}])
    );
    assert_eq!(result.request.body, Some(body));
}

#[tokio::test]
async fn tool_call_response_maps_to_a_tool_call_with_a_generated_id() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "generate",
        "tool-call",
    );
    let mut options = options();
    options.tools = vec![ToolDefinition::function(
        "weather",
        None,
        json!({"type": "object", "properties": {"location": {"type": "string"}}}),
    )];
    let result = test
        .provider
        .language_model("gemini-3-pro-preview")
        .do_generate(options)
        .await
        .unwrap();
    let call = result.content[0].as_tool_call().unwrap();
    assert_eq!(call.tool_call_id.as_str(), "id-0");
    assert_eq!(call.tool_name.as_str(), "weather");
    assert_eq!(call.input, "{\"location\":\"San Francisco\"}");
    assert!(!call.provider_executed);
    assert!(call.provider_metadata.as_ref().unwrap()["google"]["thoughtSignature"].is_string());
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["google"]["finishMessage"],
        json!("Model generated function call(s).")
    );
}

#[tokio::test]
async fn reasoning_parts_map_to_reasoning_content() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "generate",
        "reasoning",
    );
    let result = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(options())
        .await
        .unwrap();
    insta::assert_json_snapshot!("generate_reasoning_content", result.content);
    assert_eq!(result.usage.input.total, Some(12));
    assert_eq!(result.usage.input.cache_read, Some(4));
    assert_eq!(result.usage.output.reasoning, Some(40));
    assert_eq!(
        result.response.timestamp.map(|t| t.to_rfc3339()),
        Some("2026-09-14T08:00:00+00:00".to_owned())
    );
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["google"]["safetyRatings"][0]["category"],
        json!("HARM_CATEGORY_HARASSMENT")
    );
}

#[tokio::test]
async fn grounding_chunks_map_to_sources() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "generate",
        "grounding",
    );
    let result = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(options())
        .await
        .unwrap();
    assert_eq!(result.content.len(), 5);
    insta::assert_json_snapshot!("generate_grounding_content", result.content);
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["google"]["groundingMetadata"]["webSearchQueries"],
        json!(["ferrin rust sdk"])
    );
    assert!(metadata["google"]["urlContextMetadata"]["urlMetadata"].is_array());
}

#[tokio::test]
async fn code_execution_parts_map_to_provider_executed_tool_calls() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "generate",
        "code-execution",
    );
    let mut options = options();
    options.tools = vec![
        ferrin_google::GoogleTools::new()
            .code_execution()
            .definition("code_execution".into(), None),
    ];
    let result = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(options)
        .await
        .unwrap();
    insta::assert_json_snapshot!("generate_code_execution_content", result.content);
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["tools"], json!([{"codeExecution": {}}]));
}

#[tokio::test]
async fn server_tool_calls_and_responses_map_to_dynamic_tool_parts() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "generate",
        "server-tool",
    );
    let result = test
        .provider
        .language_model("gemini-3-pro-preview")
        .do_generate(options())
        .await
        .unwrap();
    insta::assert_json_snapshot!("generate_server_tool_content", result.content);
}

#[tokio::test]
async fn blocked_prompts_finish_with_content_filter() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "generate",
        "blocked",
    );
    let result = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(options())
        .await
        .unwrap();
    assert!(result.content.is_empty());
    assert_eq!(
        result.finish_reason.unified,
        FinishReasonKind::ContentFilter
    );
    assert_eq!(result.finish_reason.raw.as_deref(), Some("SAFETY"));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["google"]["promptFeedback"]["blockReason"],
        json!("SAFETY")
    );
}

#[tokio::test]
async fn inline_data_maps_to_file_content() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash-image"),
        "generate",
        "inline-image",
    );
    let mut options = options();
    options.provider_options = google_options(json!({"responseModalities": ["TEXT", "IMAGE"]}));
    let result = test
        .provider
        .language_model("gemini-2.5-flash-image")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content.len(), 2);
    let Content::File {
        data: FileData::Bytes { data },
        media_type,
        ..
    } = &result.content[1]
    else {
        panic!("expected file content, got {:?}", result.content[1]);
    };
    assert_eq!(media_type.as_str(), "image/png");
    assert_eq!(data.len(), 70);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["generationConfig"]["responseModalities"],
        json!(["TEXT", "IMAGE"])
    );
}

#[tokio::test]
async fn error_responses_map_to_api_call_errors() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "generate",
        "error-429",
    );
    let error = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(options())
        .await
        .unwrap_err();
    let ProviderError::ApiCall(error) = error else {
        panic!("expected api call error, got {error:?}");
    };
    assert_eq!(error.status_code, Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(error.is_retryable);
    assert_eq!(
        error.message,
        "You exceeded your current quota, please check your plan."
    );
    assert_eq!(
        error.data.as_ref().unwrap()["error"]["status"],
        json!("RESOURCE_EXHAUSTED")
    );
    assert_eq!(
        error
            .response_headers
            .as_ref()
            .and_then(|headers| headers.get_str("retry-after")),
        Some("34")
    );
}

#[tokio::test]
async fn custom_provider_name_reads_and_writes_both_keys() {
    let test = TestProvider::start_with(|mut settings| {
        settings.name = Some("mygemini".to_owned());
        settings
    })
    .await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "generate",
        "text",
    );
    let mut options = options();
    options.provider_options = options_under("mygemini", json!({"serviceTier": "priority"}));
    let result = test
        .provider
        .language_model("gemini-3-pro-preview")
        .do_generate(options)
        .await
        .unwrap();
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["google"], metadata["mygemini"]);
    let Content::Text {
        provider_metadata: Some(part_metadata),
        ..
    } = &result.content[0]
    else {
        panic!("expected text with metadata");
    };
    assert_eq!(part_metadata["google"], part_metadata["mygemini"]);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["serviceTier"], json!("priority"));
}
