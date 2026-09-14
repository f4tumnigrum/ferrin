//! Non-streaming Messages API calls replayed from fixtures.

use ferrin_anthropic::tools::AnthropicTools;
use ferrin_anthropic::tools::WebSearchArgs;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FileData;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::anthropic_options;

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
    ]
}

#[tokio::test]
async fn text_response_maps_content_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "text-basic");
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].as_text(), Some("Hello from Ferrin."));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.finish_reason.raw.as_deref(), Some("end_turn"));
    assert_eq!(result.usage.input.total, Some(57));
    assert_eq!(result.usage.input.no_cache, Some(42));
    assert_eq!(result.usage.input.cache_read, Some(10));
    assert_eq!(result.usage.input.cache_write, Some(5));
    assert_eq!(result.usage.output.total, Some(12));
    assert_eq!(result.response.id.as_deref(), Some("msg_text_basic"));
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("claude-sonnet-4-5")
    );
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["anthropic"]["usage"]["input_tokens"], json!(42));
    assert_eq!(
        metadata["anthropic"]["usage"]["service_tier"],
        json!("standard")
    );
    assert_eq!(metadata["anthropic"]["stopSequence"], json!(null));
    assert_eq!(metadata["anthropic"]["iterations"], json!(null));
    assert_eq!(metadata["anthropic"]["container"], json!(null));
    assert_eq!(metadata["anthropic"]["contextManagement"], json!(null));
    assert!(result.warnings.is_empty());

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["model"], json!("claude-sonnet-4-5"));
    assert_eq!(request["max_tokens"], json!(64000));
    assert_eq!(
        request["system"],
        json!([{"type": "text", "text": "You are terse."}])
    );
    assert_eq!(request["messages"][0]["role"], json!("user"));
    assert_eq!(
        request["messages"][0]["content"],
        json!([{"type": "text", "text": "Say hello"}])
    );
    assert!(request.get("stream").is_none());
}

#[tokio::test]
async fn tool_call_response_maps_to_tool_call_content() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "tool-call");
    let mut options = CallOptions::new(prompt());
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}),
    )];
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content[0].as_text(), Some("Let me check."));
    let call = result.content[1].as_tool_call().unwrap();
    assert_eq!(call.tool_call_id.as_str(), "toolu_1");
    assert_eq!(call.tool_name.as_str(), "get_weather");
    assert_eq!(call.input, "{\"city\":\"Paris\"}");
    assert!(!call.provider_executed);
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);

    let request = test.only_request();
    let body = request.body_json().unwrap();
    assert_eq!(body["tools"][0]["name"], json!("get_weather"));
    assert_eq!(body["tools"][0]["description"], json!("Weather lookup"));
    assert_eq!(body["tools"][0]["input_schema"]["type"], json!("object"));
    assert!(body.get("tool_choice").is_none());
    assert_eq!(
        request.header("anthropic-beta"),
        Some("structured-outputs-2025-11-13")
    );
}

#[tokio::test]
async fn reasoning_response_maps_thinking_and_redacted_blocks() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "reasoning");
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
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
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["signature"],
        json!("sig_1")
    );
    let Content::Reasoning {
        text,
        provider_metadata,
    } = &result.content[1]
    else {
        panic!("expected redacted reasoning, got {:?}", result.content[1]);
    };
    assert_eq!(text, "");
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["redactedData"],
        json!("redacted_1")
    );
    assert_eq!(result.content[2].as_text(), Some("It is sunny."));
    assert_eq!(result.usage.output.total, Some(40));
    assert_eq!(result.usage.output.reasoning, Some(25));
    assert_eq!(result.usage.output.text, Some(15));
}

#[tokio::test]
async fn web_search_response_maps_call_result_sources_and_citations() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "web-search");
    let mut options = CallOptions::new(prompt());
    options.tools = vec![
        AnthropicTools::new()
            .web_search_20250305(WebSearchArgs {
                max_uses: Some(3),
                ..WebSearchArgs::default()
            })
            .definition("web_search".into(), None),
    ];
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content.len(), 6, "{:#?}", result.content);
    let call = result.content[0].as_tool_call().unwrap();
    assert_eq!(call.tool_call_id.as_str(), "srvtoolu_1");
    assert_eq!(call.tool_name.as_str(), "web_search");
    assert!(call.provider_executed);
    assert!(!call.dynamic);
    assert_eq!(call.input, "{\"query\":\"ferrin rust sdk\"}");
    let Content::ToolResult(tool_result) = &result.content[1] else {
        panic!("expected tool result, got {:?}", result.content[1]);
    };
    assert_eq!(tool_result.tool_name.as_str(), "web_search");
    assert!(!tool_result.is_error);
    assert_eq!(tool_result.result[0]["pageAge"], json!("2 days ago"));
    assert_eq!(tool_result.result[0]["encryptedContent"], json!("enc_1"));
    assert!(tool_result.result[1].get("title").is_none());
    assert_eq!(tool_result.result[1]["pageAge"], json!(null));
    let Content::Source(Source::Url {
        url,
        title,
        provider_metadata,
        ..
    }) = &result.content[2]
    else {
        panic!("expected url source, got {:?}", result.content[2]);
    };
    assert_eq!(url, "https://example.test/ferrin");
    assert_eq!(title.as_deref(), Some("Ferrin"));
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["pageAge"],
        json!("2 days ago")
    );
    assert!(matches!(
        &result.content[3],
        Content::Source(Source::Url { title: None, .. })
    ));
    let Content::Text {
        text,
        provider_metadata,
    } = &result.content[4]
    else {
        panic!("expected text, got {:?}", result.content[4]);
    };
    assert_eq!(text, "Ferrin is a Rust SDK.");
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["citations"][0]["encrypted_index"],
        json!("idx_1")
    );
    let Content::Source(Source::Url {
        provider_metadata, ..
    }) = &result.content[5]
    else {
        panic!("expected citation source, got {:?}", result.content[5]);
    };
    assert_eq!(
        provider_metadata.as_ref().unwrap()["anthropic"]["citedText"],
        json!("Ferrin is a Rust SDK")
    );

    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["tools"][0],
        json!({"type": "web_search_20250305", "name": "web_search", "max_uses": 3})
    );
}

#[tokio::test]
async fn document_citations_reference_prompt_files() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "citations");
    let mut file = FilePart::new(
        FileData::Bytes {
            data: bytes::Bytes::from_static(b"%PDF-1.4 fake"),
        },
        "application/pdf",
    )
    .with_filename("report.pdf");
    file.provider_options = Some(anthropic_options(json!({"citations": {"enabled": true}})));
    let options = CallOptions::new(vec![PromptMessage::user(vec![
        UserPromptPart::File(file),
        UserPromptPart::Text(ferrin_spec::language_model::prompt::TextPart::new(
            "Summarize the report.",
        )),
    ])]);
    let result = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content.len(), 2, "{:#?}", result.content);
    let Content::Source(Source::Document {
        media_type,
        title,
        filename,
        provider_metadata,
        ..
    }) = &result.content[1]
    else {
        panic!("expected document source, got {:?}", result.content[1]);
    };
    assert_eq!(media_type.as_str(), "application/pdf");
    assert_eq!(title, "report.pdf");
    assert_eq!(filename.as_deref(), Some("report.pdf"));
    let metadata = provider_metadata.as_ref().unwrap();
    assert_eq!(metadata["anthropic"]["startPageNumber"], json!(2));
    assert_eq!(metadata["anthropic"]["endPageNumber"], json!(3));
    assert_eq!(metadata["anthropic"]["citedText"], json!("revenue grew"));

    let request = test.only_request().body_json().unwrap();
    let document = &request["messages"][0]["content"][0];
    assert_eq!(document["type"], json!("document"));
    assert_eq!(document["source"]["type"], json!("base64"));
    assert_eq!(document["source"]["media_type"], json!("application/pdf"));
    assert_eq!(document["citations"], json!({"enabled": true}));
    assert_eq!(document["title"], json!("report.pdf"));
}

#[tokio::test]
async fn json_response_tool_fallback_maps_tool_input_to_text() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "json-tool");
    let mut options = CallOptions::new(prompt());
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"]
        })),
        name: None,
        description: None,
    });
    let result = test
        .provider
        .messages("claude-3-haiku-20240307")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].as_text(), Some("{\"answer\":\"42\"}"));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);

    let request = test.only_request();
    let body = request.body_json().unwrap();
    assert_eq!(body["tools"][0]["name"], json!("json"));
    assert_eq!(
        body["tool_choice"],
        json!({"type": "any", "disable_parallel_tool_use": true})
    );
    assert!(body.get("output_config").is_none());
    assert_eq!(request.header("anthropic-beta"), None);
}

#[tokio::test]
async fn api_error_maps_to_api_call_error_with_status_and_message() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "error-400");
    let error = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    let ProviderError::ApiCall(api) = &error else {
        panic!("expected api call error, got {error:?}");
    };
    assert_eq!(api.status_code, Some(StatusCode::BAD_REQUEST));
    assert!(
        api.message.contains("max_tokens: must be greater than 0"),
        "{}",
        api.message
    );
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn rate_limit_error_is_retryable() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/messages", "messages", "error-429");
    let error = test
        .provider
        .messages("claude-sonnet-4-5")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(error.is_retryable());
}
