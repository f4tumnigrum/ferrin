//! Prompt conversion to `contents` and `systemInstruction`.

use bytes::Bytes;
use ferrin_google::capabilities::capabilities;
use ferrin_google::convert_prompt::ConvertedPrompt;
use ferrin_google::convert_prompt::convert_prompt;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
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
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::TestProvider;
use super::common::google_options;

fn convert(
    test: &TestProvider,
    model: &str,
    prompt: &[PromptMessage],
) -> Result<ConvertedPrompt, ProviderError> {
    convert_prompt(
        test.provider.config(),
        prompt,
        capabilities(model),
        &ToolNameMapping::default(),
    )
}

fn signed_text(text: &str, signature: &str) -> AssistantPromptPart {
    let mut part = TextPart::new(text);
    part.provider_options = Some(google_options(json!({"thoughtSignature": signature})));
    AssistantPromptPart::Text(part)
}

fn tool_call(
    id: &str,
    name: &str,
    input: serde_json::Value,
    options: Option<serde_json::Value>,
) -> ToolCallPart {
    ToolCallPart {
        tool_call_id: id.into(),
        tool_name: name.into(),
        input,
        provider_executed: false,
        provider_options: options.map(google_options),
    }
}

fn tool_result(id: &str, name: &str, output: ToolResultOutput) -> ToolPromptPart {
    ToolPromptPart::ToolResult(ToolResultPart {
        tool_call_id: id.into(),
        tool_name: name.into(),
        output,
        provider_options: None,
    })
}

fn file_reference(value: &str) -> ProviderReference {
    let mut reference = ProviderReference::new();
    reference.insert("google".to_owned(), value.to_owned());
    reference
}

#[tokio::test]
async fn user_and_system_messages_map_to_contents() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::system("Be brief."),
        PromptMessage::system("Answer in English."),
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("Describe these")),
            UserPromptPart::File(FilePart::new(
                FileData::Bytes {
                    data: Bytes::from_static(b"\x89PNG"),
                },
                "image/png",
            )),
            UserPromptPart::File(FilePart::new(
                FileData::Url {
                    url: Url::parse("https://example.test/paper.pdf").unwrap(),
                },
                "application/pdf",
            )),
            UserPromptPart::File(FilePart::new(
                FileData::Reference {
                    reference: file_reference(
                        "https://generativelanguage.googleapis.com/v1beta/files/abc",
                    ),
                },
                "video/mp4",
            )),
            UserPromptPart::File(FilePart::new(
                FileData::Text {
                    text: "plain notes".to_owned(),
                },
                "text/plain",
            )),
        ]),
        PromptMessage::assistant(vec![
            signed_text("Sure.", "sig-1"),
            AssistantPromptPart::Reasoning(ReasoningPart::new("thinking...")),
        ]),
    ];
    let converted = convert(&test, "gemini-2.5-flash", &prompt).unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    assert_eq!(
        converted.system_instruction,
        Some(
            json!({"parts": [{"text": "Be brief."}, {"text": "Answer in English."}]})
                .as_object()
                .unwrap()
                .clone()
        )
    );
    insta::assert_json_snapshot!("prompt_contents", converted.contents);
}

#[tokio::test]
async fn gemma_models_prepend_system_text_to_the_first_user_message() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::system("Be brief."),
        PromptMessage::user_text("Hello"),
    ];
    let converted = convert(&test, "gemma-3-27b-it", &prompt).unwrap();
    assert!(converted.system_instruction.is_none());
    assert_eq!(
        converted.contents,
        vec![json!({"role": "user", "parts": [{"text": "Be brief.\n\nHello"}]})]
    );
}

#[tokio::test]
async fn system_messages_after_the_start_and_foreign_references_are_rejected() {
    let test = TestProvider::start().await;
    let prompt = vec![
        PromptMessage::user_text("Hello"),
        PromptMessage::system("Too late."),
    ];
    let error = convert(&test, "gemini-2.5-flash", &prompt).unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
    let mut reference = ProviderReference::new();
    reference.insert("openai".to_owned(), "file-abc".to_owned());
    let prompt = vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(FileData::Reference { reference }, "application/pdf"),
    )])];
    let error = convert(&test, "gemini-2.5-flash", &prompt).unwrap_err();
    assert!(
        matches!(error, ProviderError::NoSuchProviderReference(_)),
        "{error:?}"
    );
}

fn tool_conversation(signed: bool) -> Vec<PromptMessage> {
    vec![
        PromptMessage::user_text("Weather and a chart please"),
        PromptMessage::assistant(vec![
            AssistantPromptPart::ToolCall(tool_call(
                "call-1",
                "get_weather",
                json!({"city": "Berlin"}),
                signed.then(|| json!({"thoughtSignature": "sig-call"})),
            )),
            AssistantPromptPart::ToolCall(tool_call(
                "call-2",
                "render_chart",
                json!({"kind": "bar"}),
                None,
            )),
            AssistantPromptPart::ToolCall(tool_call("call-3", "deny_me", json!({}), None)),
            AssistantPromptPart::ToolCall(tool_call("call-4", "lookup", json!({}), None)),
        ]),
        PromptMessage::tool(vec![
            tool_result(
                "call-1",
                "get_weather",
                ToolResultOutput::text("Sunny, 21C"),
            ),
            tool_result(
                "call-2",
                "render_chart",
                ToolResultOutput::Content {
                    value: vec![
                        ToolResultContentPart::Text {
                            text: "Here is the chart".to_owned(),
                            provider_options: None,
                        },
                        ToolResultContentPart::File {
                            data: FileData::Bytes {
                                data: Bytes::from_static(b"\x89PNG"),
                            },
                            media_type: "image/png".into(),
                            filename: None,
                            provider_options: None,
                        },
                    ],
                },
            ),
            tool_result(
                "call-3",
                "deny_me",
                ToolResultOutput::execution_denied(Some("not allowed".to_owned())),
            ),
            tool_result(
                "call-4",
                "lookup",
                ToolResultOutput::json(json!({"rows": [1, 2]})),
            ),
        ]),
    ]
}

#[tokio::test]
async fn tool_calls_and_results_use_the_legacy_wire_format_before_gemini_3() {
    let test = TestProvider::start().await;
    let converted = convert(&test, "gemini-2.5-flash", &tool_conversation(true)).unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    insta::assert_json_snapshot!("prompt_tools_legacy", converted.contents);
}

#[tokio::test]
async fn tool_calls_and_results_use_ids_and_signatures_on_gemini_3() {
    let test = TestProvider::start().await;
    // One signed call in the message: the others are sent as-is.
    let converted = convert(&test, "gemini-3-pro-preview", &tool_conversation(true)).unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    insta::assert_json_snapshot!("prompt_tools_gemini3", converted.contents);
    // No signed call: every call gets the skip sentinel and one warning.
    let converted = convert(&test, "gemini-3-pro-preview", &tool_conversation(false)).unwrap();
    assert_eq!(converted.warnings.len(), 1, "{:?}", converted.warnings);
    let calls = converted.contents[1]["parts"].as_array().unwrap();
    assert_eq!(calls.len(), 4);
    for call in calls {
        assert_eq!(
            call["thoughtSignature"],
            json!("skip_thought_signature_validator")
        );
    }
    insta::assert_json_snapshot!("prompt_tools_gemini3_warnings", converted.warnings);
}

#[tokio::test]
async fn generated_files_replay_their_thought_signatures() {
    use ferrin_google::output::OutputMapper;
    use ferrin_spec::Content;
    use ferrin_spec::language_model::prompt::ReasoningFilePart;

    let test = TestProvider::start().await;
    let mapper = OutputMapper::new(test.provider.config().clone(), ToolNameMapping::default());
    let mut parts = Vec::new();
    for thought in [false, true] {
        let output = mapper
            .inline_file("image/png", "aGk=", thought, Some("opaque-signature"))
            .unwrap();
        parts.push(match output {
            Content::File {
                data,
                media_type,
                provider_metadata,
                ..
            } => {
                let mut file = FilePart::new(data, media_type);
                file.provider_options = provider_metadata;
                AssistantPromptPart::File(file)
            }
            Content::ReasoningFile {
                data,
                media_type,
                provider_metadata,
            } => AssistantPromptPart::ReasoningFile(ReasoningFilePart {
                data,
                media_type,
                provider_options: provider_metadata,
            }),
            other => panic!("expected file, got {other:?}"),
        });
    }
    let converted = convert(
        &test,
        "gemini-3-pro-preview",
        &[PromptMessage::assistant(parts)],
    )
    .unwrap();
    assert_eq!(
        converted.contents,
        vec![json!({"role":"model", "parts":[
            {"inlineData":{"mimeType":"image/png", "data":"aGk="}, "thoughtSignature":"opaque-signature"},
            {"inlineData":{"mimeType":"image/png", "data":"aGk="}, "thought":true, "thoughtSignature":"opaque-signature"}
        ]})]
    );
}

#[tokio::test]
async fn generated_code_execution_roundtrips_with_its_result_and_alias() {
    use ferrin_google::output::OutputMapper;
    use ferrin_google::stream::GoogleStreamState;
    use ferrin_provider_util::http::ParseResult;
    use ferrin_provider_util::stream_driver::StreamMachine;
    use ferrin_spec::Content;
    use ferrin_spec::StreamPart;

    let test = TestProvider::start().await;
    let mapping = ToolNameMapping::default().with_pair("python", "code_execution");
    let mut mapper = OutputMapper::new(test.provider.config().clone(), mapping.clone());
    let wire = json!([
        {"executableCode":{"language":"PYTHON", "code":"print(1)"}, "thoughtSignature":"code-signature"},
        {"codeExecutionResult":{"outcome":"OUTCOME_OK", "output":"1\n"}, "thoughtSignature":"result-signature"}
    ]);
    let output = mapper
        .map_parts(
            &serde_json::from_value::<Vec<ferrin_google::api_types::Part>>(wire.clone()).unwrap(),
        )
        .unwrap();
    let mapper = OutputMapper::new(test.provider.config().clone(), mapping.clone());
    let mut state = GoogleStreamState::new(mapper);
    let raw =
        json!({"candidates":[{"content":{"role":"model", "parts":wire}, "finishReason":"STOP"}]});
    let streamed = state
        .handle(
            ParseResult::Ok {
                value: serde_json::from_value(raw.clone()).unwrap(),
                raw,
            },
            false,
        )
        .into_iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some(Content::ToolCall(call)),
            StreamPart::ToolResult(result) => Some(Content::ToolResult(result)),
            _ => None,
        })
        .collect();
    for output in [output, streamed] {
        let mut parts = Vec::new();
        for content in output {
            parts.push(match content {
                Content::ToolCall(call) => {
                    assert_eq!(call.tool_name.as_str(), "python");
                    AssistantPromptPart::ToolCall(ToolCallPart {
                        tool_call_id: call.tool_call_id,
                        tool_name: call.tool_name,
                        input: serde_json::from_str(&call.input).unwrap(),
                        provider_executed: call.provider_executed,
                        provider_options: call.provider_metadata,
                    })
                }
                Content::ToolResult(result) => {
                    assert_eq!(result.tool_name.as_str(), "python");
                    AssistantPromptPart::ToolResult(ToolResultPart {
                        tool_call_id: result.tool_call_id,
                        tool_name: result.tool_name,
                        output: ToolResultOutput::Json {
                            value: result.result,
                            provider_options: None,
                        },
                        provider_options: result.provider_metadata,
                    })
                }
                other => panic!("expected code execution content, got {other:?}"),
            });
        }
        let converted = convert_prompt(
            test.provider.config(),
            &[PromptMessage::assistant(parts)],
            capabilities("gemini-3-pro-preview"),
            &mapping,
        )
        .unwrap();
        assert_eq!(
            converted.contents,
            vec![json!({"role":"model", "parts":wire})]
        );
        assert!(converted.warnings.is_empty());
    }
}
