//! Prompt conversion to Messages API `system` and `messages`.

use ferrin_anthropic::convert_prompt::convert_prompt;
use ferrin_anthropic::prepare_tools::tool_name_mapping;
use ferrin_spec::FileData;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ReasoningPart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::anthropic_options;

fn convert(
    test: &TestProvider,
    prompt: &[PromptMessage],
    send_reasoning: bool,
) -> Result<ferrin_anthropic::convert_prompt::ConvertedPrompt, ProviderError> {
    convert_prompt(
        test.provider.config(),
        prompt,
        &tool_name_mapping(&[]),
        send_reasoning,
    )
}

fn reasoning(text: &str, options: serde_json::Value) -> AssistantPromptPart {
    let mut part = ReasoningPart::new(text);
    part.provider_options = Some(anthropic_options(options));
    AssistantPromptPart::Reasoning(part)
}

fn tool_call(
    id: &str,
    name: &str,
    input: serde_json::Value,
    provider_executed: bool,
) -> ToolCallPart {
    ToolCallPart {
        tool_call_id: id.into(),
        tool_name: name.into(),
        input,
        provider_executed,
        provider_options: None,
    }
}

fn tool_result(id: &str, name: &str, output: ToolResultOutput) -> ToolResultPart {
    ToolResultPart {
        tool_call_id: id.into(),
        tool_name: name.into(),
        output,
        provider_options: None,
    }
}

#[tokio::test]
async fn prompt_conversion_snapshot() {
    let test = TestProvider::start().await;
    let mut pdf = FilePart::new(
        FileData::Bytes {
            data: bytes::Bytes::from_static(b"%PDF-1.4 fake"),
        },
        "application/pdf",
    )
    .with_filename("report.pdf");
    pdf.provider_options = Some(anthropic_options(
        json!({"citations": {"enabled": true}, "context": "Quarterly report"}),
    ));
    let prompt = vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("Describe these:")),
            UserPromptPart::File(FilePart::new(
                FileData::Bytes {
                    data: bytes::Bytes::from_static(&[0xFF, 0xD8, 0xFF, 0xE0]),
                },
                "image/jpeg",
            )),
            UserPromptPart::File(pdf),
            UserPromptPart::File(FilePart::new(
                FileData::Url {
                    url: url::Url::parse("https://example.test/cat.png").unwrap(),
                },
                "image/png",
            )),
            UserPromptPart::File(FilePart::new(
                FileData::Text {
                    text: "plain notes".to_owned(),
                },
                "text/plain",
            )),
        ]),
        PromptMessage::assistant(vec![
            reasoning("Thinking about it.", json!({"signature": "sig_1"})),
            reasoning("", json!({"redactedData": "red_1"})),
            AssistantPromptPart::Text(TextPart::new("Let me look.")),
            AssistantPromptPart::ToolCall(tool_call(
                "toolu_1",
                "get_weather",
                json!({"city": "Paris"}),
                false,
            )),
        ]),
        PromptMessage::tool(vec![ToolPromptPart::ToolResult(tool_result(
            "toolu_1",
            "get_weather",
            ToolResultOutput::json(json!({"temperature": 21})),
        ))]),
        PromptMessage::User {
            content: vec![UserPromptPart::Text(TextPart::new("Thanks"))],
            provider_options: Some(anthropic_options(
                json!({"cacheControl": {"type": "ephemeral", "ttl": "5m"}}),
            )),
        },
        PromptMessage::assistant_text("You are welcome.  \n"),
    ];
    let converted = convert(&test, &prompt, true).unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    let betas: Vec<&String> = converted.betas.iter().collect();
    insta::assert_json_snapshot!(
        "prompt_conversion",
        json!({"system": converted.system, "messages": converted.messages, "betas": betas})
    );
}

#[tokio::test]
async fn reasoning_is_dropped_with_a_warning_when_disabled() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::user_text("Hello"),
        PromptMessage::assistant(vec![
            reasoning("Thinking.", json!({"signature": "sig_1"})),
            AssistantPromptPart::Text(TextPart::new("Hi")),
        ]),
        PromptMessage::user_text("Again"),
    ];
    let converted = convert(&test, &prompt, false).unwrap();
    assert_eq!(converted.warnings.len(), 1, "{:?}", converted.warnings);
    assert_eq!(
        converted.messages[1]["content"],
        json!([{"type": "text", "text": "Hi"}])
    );
}

#[tokio::test]
async fn unsupported_media_and_foreign_references_are_rejected() {
    let test = TestProvider::start().await;
    let audio = vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(
            FileData::Bytes {
                data: bytes::Bytes::from_static(b"RIFF....WAVE"),
            },
            "audio/wav",
        ),
    )])];
    let error = convert(&test, &audio, true).unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );

    let mut reference = ProviderReference::new();
    reference.insert("openai".to_owned(), "file-1".to_owned());
    let foreign = vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(FileData::Reference { reference }, "application/pdf"),
    )])];
    let error = convert(&test, &foreign, true).unwrap_err();
    assert!(
        matches!(error, ProviderError::NoSuchProviderReference(_)),
        "{error:?}"
    );

    let mut own = ProviderReference::new();
    own.insert("anthropic".to_owned(), "file_abc".to_owned());
    let referenced = vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(FileData::Reference { reference: own }, "application/pdf"),
    )])];
    let converted = convert(&test, &referenced, true).unwrap();
    assert_eq!(
        converted.messages[0]["content"][0]["source"],
        json!({"type": "file", "file_id": "file_abc"})
    );
    assert!(converted.betas.contains("files-api-2025-04-14"));
}

#[tokio::test]
async fn tool_results_map_every_output_kind() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::user_text("Go"),
        PromptMessage::assistant(vec![
            AssistantPromptPart::ToolCall(tool_call("a", "one", json!({}), false)),
            AssistantPromptPart::ToolCall(tool_call("b", "two", json!({}), false)),
            AssistantPromptPart::ToolCall(tool_call("c", "three", json!({}), false)),
        ]),
        PromptMessage::tool(vec![
            ToolPromptPart::ToolResult(tool_result(
                "a",
                "one",
                ToolResultOutput::execution_denied(None),
            )),
            ToolPromptPart::ToolResult(tool_result(
                "b",
                "two",
                ToolResultOutput::error_text("boom"),
            )),
            ToolPromptPart::ToolResult(tool_result("c", "three", ToolResultOutput::text("fine"))),
        ]),
    ];
    let converted = convert(&test, &prompt, true).unwrap();
    let results = &converted.messages[2]["content"];
    assert_eq!(
        results[0],
        json!({"type": "tool_result", "tool_use_id": "a", "content": "Tool call execution denied.", "is_error": true})
    );
    assert_eq!(
        results[1],
        json!({"type": "tool_result", "tool_use_id": "b", "content": "boom", "is_error": true})
    );
    assert_eq!(
        results[2],
        json!({"type": "tool_result", "tool_use_id": "c", "content": "fine"})
    );
}

#[tokio::test]
async fn provider_executed_calls_and_results_round_trip() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::user_text("Search"),
        PromptMessage::assistant(vec![
            AssistantPromptPart::ToolCall(tool_call(
                "srvtoolu_1",
                "web_search",
                json!({"query": "ferrin"}),
                true,
            )),
            AssistantPromptPart::ToolResult(tool_result(
                "srvtoolu_1",
                "web_search",
                ToolResultOutput::json(json!([{
                    "url": "https://example.test",
                    "title": "Example",
                    "pageAge": null,
                    "encryptedContent": "enc",
                    "type": "web_search_result"
                }])),
            )),
            AssistantPromptPart::Text(TextPart::new("Found it.")),
        ]),
        PromptMessage::user_text("Thanks"),
    ];
    let converted = convert(&test, &prompt, true).unwrap();
    let content = &converted.messages[1]["content"];
    assert_eq!(content[0]["type"], json!("server_tool_use"));
    assert_eq!(content[0]["name"], json!("web_search"));
    assert_eq!(content[1]["type"], json!("web_search_tool_result"));
    assert_eq!(content[1]["content"][0]["encrypted_content"], json!("enc"));
    assert_eq!(content[1]["content"][0]["page_age"], json!(null));
    assert_eq!(content[2]["type"], json!("text"));
}
