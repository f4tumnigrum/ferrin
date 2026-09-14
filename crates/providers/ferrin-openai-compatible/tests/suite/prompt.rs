//! Prompt conversion of the chat model.

use bytes::Bytes;
use ferrin_openai_compatible::chat::convert_prompt::convert_prompt;
use ferrin_spec::FileData;
use ferrin_spec::PromptMessage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ReasoningPart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;
use url::Url;

use super::common::example_options;
use super::common::options_under;

fn bytes_file(data: &'static [u8], media_type: &str) -> FilePart {
    FilePart::new(
        FileData::Bytes {
            data: Bytes::from_static(data),
        },
        media_type,
    )
}

fn url_file(url: &str, media_type: &str) -> FilePart {
    FilePart::new(
        FileData::Url {
            url: Url::parse(url).unwrap(),
        },
        media_type,
    )
}

#[test]
fn prompt_conversion_covers_files_tool_calls_and_results() {
    let mut system = PromptMessage::system("Be brief.");
    if let PromptMessage::System {
        provider_options, ..
    } = &mut system
    {
        *provider_options = Some(options_under("openaiCompatible", json!({"name": "sys"})));
    }
    let mut tool_result = ToolResultPart {
        tool_call_id: "call_1".into(),
        tool_name: "get_weather".into(),
        output: ToolResultOutput::json(json!({"temperature": 21})),
        provider_options: None,
    };
    tool_result.provider_options = Some(options_under(
        "openaiCompatible",
        json!({"name": "get_weather"}),
    ));
    let prompt = vec![
        system,
        PromptMessage::user_text("Single text"),
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("Look")),
            UserPromptPart::File(bytes_file(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0], "image/*")),
            UserPromptPart::File(url_file("https://example.test/a.png", "image/png")),
            UserPromptPart::File(url_file("https://example.test/a.mp4", "video/mp4")),
            UserPromptPart::File(bytes_file(b"RIFF....WAVE", "audio/wav")),
            UserPromptPart::File(bytes_file(b"ID3....", "audio/mpeg")),
            UserPromptPart::File(
                bytes_file(b"%PDF-1.4", "application/pdf").with_filename("notes.pdf"),
            ),
            UserPromptPart::File(bytes_file(b"%PDF-1.4", "application/pdf")),
            UserPromptPart::File(bytes_file(b"plain text", "text/plain")),
            UserPromptPart::File(url_file("https://example.test/a.txt", "text/plain")),
        ]),
        PromptMessage::assistant(vec![
            AssistantPromptPart::Reasoning(ReasoningPart::new("Because")),
            AssistantPromptPart::Text(TextPart::new("Sure.")),
            AssistantPromptPart::ToolCall(ToolCallPart {
                tool_call_id: "call_1".into(),
                tool_name: "get_weather".into(),
                input: json!({"city": "Paris"}),
                provider_executed: false,
                provider_options: Some(example_options(json!({"thoughtSignature": "sig-1"}))),
            }),
        ]),
        PromptMessage::assistant(vec![AssistantPromptPart::ToolCall(ToolCallPart {
            tool_call_id: "call_2".into(),
            tool_name: "get_time".into(),
            input: json!({}),
            provider_executed: false,
            provider_options: Some(options_under(
                "google",
                json!({"thoughtSignature": "sig-2"}),
            )),
        })]),
        PromptMessage::tool(vec![
            ToolPromptPart::ToolResult(tool_result),
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "call_2".into(),
                tool_name: "get_time".into(),
                output: ToolResultOutput::Text {
                    value: "12:00".to_owned(),
                    provider_options: None,
                },
                provider_options: None,
            }),
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "call_3".into(),
                tool_name: "delete".into(),
                output: ToolResultOutput::ExecutionDenied {
                    reason: None,
                    provider_options: None,
                },
                provider_options: None,
            }),
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "call_4".into(),
                tool_name: "search".into(),
                output: ToolResultOutput::Content {
                    value: vec![ToolResultContentPart::Text {
                        text: "found".to_owned(),
                        provider_options: None,
                    }],
                },
                provider_options: None,
            }),
        ]),
    ];
    let converted = convert_prompt(&prompt, "example").unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    insta::assert_json_snapshot!("prompt_conversion", converted.messages);
}

#[test]
fn unsupported_file_parts_are_rejected() {
    let cases = [
        (
            url_file("https://example.test/a.wav", "audio/wav"),
            "audio file parts with URLs",
        ),
        (
            url_file("https://example.test/a.pdf", "application/pdf"),
            "PDF file parts with URLs",
        ),
        (
            bytes_file(b"OggS", "audio/ogg"),
            "audio media type audio/ogg",
        ),
        (
            bytes_file(b"PK..", "application/zip"),
            "file part media type application/zip",
        ),
        (
            bytes_file(b"gltf", "model/gltf+json"),
            "file part media type model/gltf+json",
        ),
        (
            FilePart::new(
                FileData::Text {
                    text: "plain".to_owned(),
                },
                "text/plain",
            ),
            "text file parts",
        ),
    ];
    for (file, expected) in cases {
        let prompt = vec![PromptMessage::user(vec![UserPromptPart::File(file)])];
        let error = convert_prompt(&prompt, "example").unwrap_err();
        assert!(
            matches!(error, ProviderError::UnsupportedFunctionality(_)),
            "{expected}: {error:?}"
        );
        assert!(error.to_string().contains(expected), "{expected}: {error}");
    }
}
