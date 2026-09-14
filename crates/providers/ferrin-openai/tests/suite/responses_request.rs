//! Request body assembly of the Responses API: snapshots and warnings.

use ferrin_openai::responses::request::prepare_request;
use ferrin_spec::CallOptions;
use ferrin_spec::PromptMessage;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::TestProvider;
use super::common::openai_options;

fn features(warnings: &[Warning]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| match warning {
            Warning::Unsupported { feature, .. } | Warning::Compatibility { feature, .. } => {
                feature.as_str()
            }
            Warning::Deprecated { setting, .. } => setting.as_str(),
            Warning::Other { message } => message.as_str(),
            _ => "?",
        })
        .collect()
}

#[tokio::test]
async fn basic_request_snapshot() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![
        PromptMessage::system("Be brief."),
        PromptMessage::user_text("Hello"),
        PromptMessage::assistant_text("Hi!"),
        PromptMessage::user_text("How are you?"),
    ]);
    options.max_output_tokens = Some(100);
    options.temperature = Some(0.3);
    options.top_p = Some(0.9);
    let prepared = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap();
    assert!(prepared.warnings.is_empty());
    insta::assert_json_snapshot!("responses_request_basic", prepared.body);
}

#[tokio::test]
async fn unsupported_settings_produce_warnings_and_are_dropped() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.top_k = Some(5);
    options.seed = Some(7);
    options.presence_penalty = Some(0.1);
    options.frequency_penalty = Some(0.2);
    options.stop_sequences = Some(vec!["END".to_owned()]);
    let prepared = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap();
    assert_eq!(
        features(&prepared.warnings),
        vec![
            "topK",
            "seed",
            "presencePenalty",
            "frequencyPenalty",
            "stopSequences"
        ]
    );
    let body = serde_json::to_value(&prepared.body).unwrap();
    assert!(body.get("seed").is_none());
}

#[tokio::test]
async fn reasoning_models_drop_sampling_parameters_and_use_developer_role() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![
        PromptMessage::system("Be brief."),
        PromptMessage::user_text("Hello"),
    ]);
    options.temperature = Some(0.5);
    options.top_p = Some(0.5);
    options.reasoning = ReasoningEffort::High;
    let prepared = prepare_request(test.provider.config(), "o3", &options).unwrap();
    assert_eq!(features(&prepared.warnings), vec!["temperature", "topP"]);
    let body = serde_json::to_value(&prepared.body).unwrap();
    assert!(body.get("temperature").is_none());
    assert_eq!(body["input"][0]["role"], json!("developer"));
    assert_eq!(body["reasoning"]["effort"], json!("high"));
    assert_eq!(body["reasoning"]["summary"], json!("detailed"));
}

#[tokio::test]
async fn provider_options_snapshot() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options = openai_options(json!({
        "instructions": "Answer in French.",
        "parallelToolCalls": false,
        "store": false,
        "user": "user-1",
        "metadata": {"trace": "abc"},
        "serviceTier": "priority",
        "textVerbosity": "low",
        "logprobs": 3,
        "promptCacheKey": "cache-1",
        "safetyIdentifier": "safe-1",
        "truncation": "auto",
        "maxToolCalls": 4,
        "reasoningEffort": "low",
        "reasoningSummary": "auto",
        "include": ["file_search_call.results"]
    }));
    let prepared = prepare_request(test.provider.config(), "gpt-5", &options).unwrap();
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("responses_request_provider_options", prepared.body);
}

#[tokio::test]
async fn unknown_provider_option_is_an_invalid_argument() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options = openai_options(json!({"notAnOption": true}));
    let error = prepare_request(test.provider.config(), "gpt-5", &options).unwrap_err();
    assert!(
        matches!(error, ferrin_spec::error::ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn json_schema_response_format_and_tools_snapshot() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Weather?")]);
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"],
            "additionalProperties": false
        })),
        name: Some("answer".to_owned()),
        description: Some("The answer".to_owned()),
    });
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"], "additionalProperties": false}),
    )];
    options.tool_choice = Some(ToolChoice::Tool {
        tool_name: "get_weather".into(),
    });
    let prepared = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap();
    assert!(prepared.warnings.is_empty());
    insta::assert_json_snapshot!("responses_request_schema_and_tools", prepared.body);
}

#[tokio::test]
async fn flex_service_tier_is_rejected_on_unsupported_models() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options = openai_options(json!({"serviceTier": "flex"}));
    let prepared = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap();
    assert_eq!(features(&prepared.warnings), vec!["serviceTier"]);
    let body = serde_json::to_value(&prepared.body).unwrap();
    assert!(body.get("service_tier").is_none());
}

#[tokio::test]
async fn file_parts_convert_to_input_image_and_input_file() {
    let test = TestProvider::start().await;
    let options = CallOptions::new(vec![PromptMessage::user(vec![
        UserPromptPart::Text(TextPart::new("Describe")),
        UserPromptPart::File(FilePart::new(
            ferrin_spec::FileData::Url {
                url: Url::parse("https://example.test/cat.png").unwrap(),
            },
            "image/png",
        )),
        UserPromptPart::File(FilePart::new(
            ferrin_spec::FileData::Bytes {
                data: bytes::Bytes::from_static(b"%PDF-1.4 fake"),
            },
            "application/pdf",
        )),
    ])]);
    let prepared = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap();
    let body = serde_json::to_value(&prepared.body).unwrap();
    let content = &body["input"][0]["content"];
    assert_eq!(content[1]["type"], json!("input_image"));
    assert_eq!(
        content[1]["image_url"],
        json!("https://example.test/cat.png")
    );
    assert_eq!(content[2]["type"], json!("input_file"));
    assert_eq!(content[2]["filename"], json!("part-2.pdf"));
    assert!(
        content[2]["file_data"]
            .as_str()
            .unwrap()
            .starts_with("data:application/pdf;base64,")
    );
}

#[tokio::test]
async fn unsupported_file_media_type_is_rejected() {
    let test = TestProvider::start().await;
    let options = CallOptions::new(vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(
            ferrin_spec::FileData::Bytes {
                data: bytes::Bytes::from_static(b"plain"),
            },
            "text/csv",
        ),
    )])]);
    let error = prepare_request(test.provider.config(), "gpt-4.1", &options).unwrap_err();
    assert!(
        matches!(
            error,
            ferrin_spec::error::ProviderError::UnsupportedFunctionality(_)
        ),
        "{error:?}"
    );
}
