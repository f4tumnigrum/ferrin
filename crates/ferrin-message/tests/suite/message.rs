use bytes::Bytes;
use ferrin_message::AssistantPart;
use ferrin_message::Message;
use ferrin_message::MessagesExt;
use ferrin_message::Role;
use ferrin_message::ToolApprovalRequest;
use ferrin_message::ToolApprovalResponse;
use ferrin_message::ToolCallPart;
use ferrin_message::ToolPart;
use ferrin_message::ToolResultOutput;
use ferrin_message::ToolResultPart;
use ferrin_message::UserContent;
use ferrin_message::UserPart;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn constructors_and_serialization() {
    let messages = vec![
        Message::system("Be terse."),
        Message::user("Describe this image."),
        Message::user_parts([
            UserPart::text("And this document:"),
            UserPart::file_bytes(Bytes::from_static(b"%PDF"), "application/pdf")
                .with_filename("report.pdf"),
            UserPart::image_url("https://example.com/cat.png".parse().unwrap()),
        ]),
        Message::assistant_parts([
            AssistantPart::reasoning("thinking"),
            AssistantPart::ToolCall(ToolCallPart {
                tool_call_id: "call_1".into(),
                tool_name: "weather".into(),
                input: json!({ "city": "Paris" }),
                provider_executed: false,
                provider_options: None,
            }),
            AssistantPart::ToolApprovalRequest(
                ToolApprovalRequest::new("approval_1", "call_1").with_reason("dangerous"),
            ),
        ]),
        Message::tool([
            ToolPart::ToolResult(ToolResultPart {
                tool_call_id: "call_1".into(),
                tool_name: "weather".into(),
                output: ToolResultOutput::text("sunny"),
                provider_options: None,
            }),
            ToolPart::ToolApprovalResponse(ToolApprovalResponse::approved("approval_1")),
        ]),
    ];
    let value = serde_json::to_value(&messages).unwrap();
    assert_eq!(
        value,
        json!([
            { "role": "system", "content": "Be terse." },
            { "role": "user", "content": "Describe this image." },
            { "role": "user", "content": [
                { "type": "text", "text": "And this document:" },
                { "type": "file", "data": { "type": "data", "data": "JVBERg==" },
                  "media_type": "application/pdf", "filename": "report.pdf" },
                { "type": "image", "image": { "type": "url", "url": "https://example.com/cat.png" } },
            ]},
            { "role": "assistant", "content": [
                { "type": "reasoning", "text": "thinking" },
                { "type": "tool-call", "tool_call_id": "call_1", "tool_name": "weather",
                  "input": { "city": "Paris" } },
                { "type": "tool-approval-request", "approval_id": "approval_1",
                  "tool_call_id": "call_1", "reason": "dangerous" },
            ]},
            { "role": "tool", "content": [
                { "type": "tool-result", "tool_call_id": "call_1", "tool_name": "weather",
                  "output": { "type": "text", "value": "sunny" } },
                { "type": "tool-approval-response", "approval_id": "approval_1", "approved": true },
            ]},
        ])
    );
    let parsed: Vec<Message> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, messages);

    let roles: Vec<Role> = messages.iter().map(Message::role).collect();
    assert_eq!(
        roles,
        vec![
            Role::System,
            Role::User,
            Role::User,
            Role::Assistant,
            Role::Tool
        ]
    );
    assert_eq!(Role::Assistant.to_string(), "assistant");
}

#[test]
fn content_helpers() {
    let text = UserContent::from("hi");
    assert_eq!(text.as_text(), Some("hi"));
    assert_eq!(text.into_parts(), vec![UserPart::text("hi")]);
    assert!(Message::user("").is_empty());
    assert!(Message::tool(Vec::<ToolPart>::new()).is_empty());
    assert!(!Message::assistant("x").is_empty());

    let with_options = Message::system("s").with_provider_options(
        [("openai".to_owned(), serde_json::Map::new())]
            .into_iter()
            .collect(),
    );
    assert!(
        with_options
            .provider_options()
            .unwrap()
            .contains_key("openai")
    );
}

#[test]
fn approval_responses_append_to_trailing_tool_message() {
    let mut history = vec![
        Message::user("delete it"),
        Message::assistant_parts([AssistantPart::ToolApprovalRequest(
            ToolApprovalRequest::new("approval_1", "call_1"),
        )]),
    ];
    assert_eq!(history.pending_approval_requests().len(), 1);

    history.push_approval_response(ToolApprovalResponse::denied("approval_1").with_reason("no"));
    assert_eq!(history.len(), 3);
    assert_eq!(history.pending_approval_requests().len(), 0);

    history.push_approval_response(ToolApprovalResponse::approved("approval_2"));
    assert_eq!(history.len(), 3);
    assert_eq!(history[2].as_tool().unwrap().content.len(), 2);
}
