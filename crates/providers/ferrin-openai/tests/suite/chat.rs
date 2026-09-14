//! Chat Completions API: generate, stream, request assembly.

use ferrin_openai::chat::convert_prompt::convert_prompt;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::openai_options;
use super::common::without_raw_usage;

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
    ]
}

#[tokio::test]
async fn generate_maps_text_sources_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let result = test
        .provider
        .chat("gpt-4o")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    assert_eq!(result.content[0].as_text(), Some("Hello from chat."));
    let Content::Source(ferrin_spec::language_model::Source::Url { url, title, id, .. }) =
        &result.content[1]
    else {
        panic!("expected url source, got {:?}", result.content[1]);
    };
    assert_eq!(url, "https://example.com/a");
    assert_eq!(title.as_deref(), Some("Example A"));
    assert_eq!(id, "id-0");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.usage.input.total, Some(20));
    assert_eq!(result.usage.input.cache_read, Some(5));
    assert_eq!(result.usage.output.reasoning, Some(2));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["acceptedPredictionTokens"], json!(1));
    assert_eq!(result.response.id.as_deref(), Some("chatcmpl_1"));

    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["messages"][0]["role"], json!("system"));
    assert_eq!(
        request["messages"][1],
        json!({"role": "user", "content": "Say hello"})
    );
}

#[tokio::test]
async fn generate_maps_tool_calls() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "tool-call");
    let mut options = CallOptions::new(prompt());
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        Some("Weather".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )];
    let result = test
        .provider
        .chat("gpt-4o")
        .do_generate(options)
        .await
        .unwrap();
    let call = result.content[0].as_tool_call().unwrap();
    assert_eq!(call.tool_call_id.as_str(), "call_c1");
    assert_eq!(call.input, "{\"city\":\"Paris\"}");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["tools"][0]["function"]["name"],
        json!("get_weather")
    );
    assert_eq!(request["tools"][0]["function"]["strict"], json!(true));
}

#[tokio::test]
async fn stream_emits_text_and_finish() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/chat/completions",
        "chat",
        "text-basic-stream",
    );
    let result = test
        .provider
        .chat("gpt-4o")
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    insta::assert_json_snapshot!("chat_stream_text", parts);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream"], json!(true));
    assert_eq!(request["stream_options"]["include_usage"], json!(true));
}

#[tokio::test]
async fn stream_tracks_tool_call_deltas() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/chat/completions",
        "chat",
        "tool-call-stream",
    );
    let result = test
        .provider
        .chat("gpt-4o")
        .do_stream(CallOptions::new(prompt()))
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
    assert_eq!(call.tool_call_id.as_str(), "call_cs1");
    assert_eq!(call.input, "{\"city\":\"Paris\"}");
    insta::assert_json_snapshot!("chat_stream_tool_call", parts);
}

#[tokio::test]
async fn early_error_frame_fails_the_stream_call() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "error-early");
    let error = test
        .provider
        .chat("gpt-4o")
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::ApiCall(_)), "{error:?}");
    assert!(error.to_string().contains("overloaded"), "{error}");
}

#[tokio::test]
async fn unauthorized_response_is_an_api_call_error() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "error-401");
    let error = test
        .provider
        .chat("gpt-4o")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), Some(StatusCode::UNAUTHORIZED));
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn reasoning_models_move_max_tokens_and_drop_sampling() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(prompt());
    options.max_output_tokens = Some(50);
    options.temperature = Some(0.2);
    options.frequency_penalty = Some(0.1);
    options.provider_options = openai_options(json!({"logprobs": true, "logitBias": {"1": 2.0}}));
    let prepared = test
        .provider
        .chat("o3-mini")
        .prepare_request(&options)
        .unwrap();
    let body = serde_json::to_value(&prepared.body).unwrap();
    assert_eq!(body["max_completion_tokens"], json!(50));
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("temperature").is_none());
    assert!(body.get("logprobs").is_none());
    assert!(body.get("logit_bias").is_none());
    assert_eq!(body["messages"][0]["role"], json!("developer"));
    let messages: Vec<String> = prepared
        .warnings
        .iter()
        .map(|warning| match warning {
            Warning::Unsupported { feature, .. } => feature.clone(),
            Warning::Other { message } => message.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        messages,
        vec![
            "temperature",
            "logprobs is not supported for reasoning models",
            "frequencyPenalty",
            "logitBias is not supported for reasoning models",
        ]
    );
}

#[tokio::test]
async fn request_snapshot_with_json_schema_and_options() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(prompt());
    options.response_format = Some(ferrin_spec::ResponseFormat::Json {
        schema: Some(json!({"type": "object", "properties": {"a": {"type": "string"}}})),
        name: None,
        description: None,
    });
    options.provider_options = openai_options(json!({
        "user": "u1",
        "parallelToolCalls": false,
        "store": true,
        "serviceTier": "auto",
        "promptCacheKey": "k",
        "logprobs": 2
    }));
    let prepared = test
        .provider
        .chat("gpt-4o")
        .prepare_request(&options)
        .unwrap();
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("chat_request_schema_and_options", prepared.body);
}

#[test]
fn prompt_conversion_covers_files_tool_calls_and_results() {
    let prompt = vec![
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("Look")),
            UserPromptPart::File(FilePart::new(
                ferrin_spec::FileData::Bytes {
                    data: bytes::Bytes::from_static(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
                },
                "image/jpeg",
            )),
            UserPromptPart::File(
                FilePart::new(
                    ferrin_spec::FileData::Bytes {
                        data: bytes::Bytes::from_static(b"RIFF....WAVE"),
                    },
                    "audio/wav",
                )
                .with_filename("clip.wav"),
            ),
        ]),
        PromptMessage::assistant(vec![AssistantPromptPart::ToolCall(ToolCallPart {
            tool_call_id: "call_1".into(),
            tool_name: "get_weather".into(),
            input: json!({"city": "Paris"}),
            provider_executed: false,
            provider_options: None,
        })]),
        PromptMessage::tool(vec![ToolPromptPart::ToolResult(ToolResultPart {
            tool_call_id: "call_1".into(),
            tool_name: "get_weather".into(),
            output: ToolResultOutput::json(json!({"temperature": 21})),
            provider_options: None,
        })]),
    ];
    let converted = convert_prompt(
        &prompt,
        ferrin_openai::capabilities::SystemMessageMode::System,
        "openai",
    )
    .unwrap();
    assert!(converted.warnings.is_empty());
    insta::assert_json_snapshot!("chat_prompt_conversion", converted.messages);
}

#[test]
fn text_file_parts_are_unsupported() {
    let prompt = vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(
            ferrin_spec::FileData::Text {
                text: "plain".to_owned(),
            },
            "text/plain",
        ),
    )])];
    let error = convert_prompt(
        &prompt,
        ferrin_openai::capabilities::SystemMessageMode::System,
        "openai",
    )
    .unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
}
