use bytes::Bytes;
use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::FileData;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolApprovalResponsePart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;

#[test]
fn messages_serialize_with_role_and_type_tags() {
    let prompt = vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("Describe this")),
            UserPromptPart::File(
                FilePart::new(FileData::bytes(Bytes::from_static(b"\x89PNG")), "image/png")
                    .with_filename("a.png"),
            ),
        ]),
        PromptMessage::assistant(vec![AssistantPromptPart::ToolCall(ToolCallPart {
            tool_call_id: "call_1".into(),
            tool_name: "weather".into(),
            input: json!({ "city": "Paris" }),
            provider_executed: false,
            provider_options: None,
        })]),
        PromptMessage::tool(vec![
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "call_1".into(),
                tool_name: "weather".into(),
                output: ToolResultOutput::json(json!({ "temp": 21 })),
                provider_options: None,
            }),
            ToolPromptPart::ToolApprovalResponse(ToolApprovalResponsePart {
                approval_id: "appr_1".into(),
                approved: true,
                reason: None,
                provider_options: None,
            }),
        ]),
    ];

    let value = serde_json::to_value(&prompt).unwrap();
    assert_eq!(
        value,
        json!([
            { "role": "system", "content": "You are terse." },
            { "role": "user", "content": [
                { "type": "text", "text": "Describe this" },
                { "type": "file", "data": { "type": "data", "data": "iVBORw==" }, "media_type": "image/png", "filename": "a.png" }
            ]},
            { "role": "assistant", "content": [
                { "type": "tool-call", "tool_call_id": "call_1", "tool_name": "weather",
                  "input": { "city": "Paris" } }
            ]},
            { "role": "tool", "content": [
                { "type": "tool-result", "tool_call_id": "call_1", "tool_name": "weather",
                  "output": { "type": "json", "value": { "temp": 21 } } },
                { "type": "tool-approval-response", "approval_id": "appr_1", "approved": true }
            ]},
        ])
    );

    let parsed: Vec<PromptMessage> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, prompt);
}

#[test]
fn tool_result_output_variants_round_trip() {
    let outputs = vec![
        ToolResultOutput::text("ok"),
        ToolResultOutput::error_text("boom"),
        ToolResultOutput::error_json(json!({ "code": 1 })),
        ToolResultOutput::execution_denied(Some("policy".to_owned())),
        ToolResultOutput::Content { value: vec![] },
    ];
    let value = serde_json::to_value(&outputs).unwrap();
    assert_eq!(
        value,
        json!([
            { "type": "text", "value": "ok" },
            { "type": "error-text", "value": "boom" },
            { "type": "error-json", "value": { "code": 1 } },
            { "type": "execution-denied", "reason": "policy" },
            { "type": "content", "value": [] },
        ])
    );
    let parsed: Vec<ToolResultOutput> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, outputs);
    assert!(outputs[1].is_error());
    assert!(!outputs[0].is_error());
}

#[test]
fn role_accessor_matches_variant() {
    assert_eq!(PromptMessage::system("x").role(), "system");
    assert_eq!(PromptMessage::user_text("x").role(), "user");
    assert_eq!(PromptMessage::assistant_text("x").role(), "assistant");
    assert_eq!(PromptMessage::tool(vec![]).role(), "tool");
}
