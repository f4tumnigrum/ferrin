use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::language_model::content::Content;
use ferrin_spec::language_model::content::CustomKind;
use ferrin_spec::language_model::content::ProviderToolResult;
use ferrin_spec::language_model::content::Source;
use ferrin_spec::language_model::content::ToolCall;

#[test]
fn content_serializes_with_type_tag_and_nested_tags() {
    let content = vec![
        Content::text("hi"),
        Content::ToolCall(ToolCall::new("call_1", "weather", r#"{"city":"Paris"}"#)),
        Content::Source(Source::Url {
            id: "s1".to_owned(),
            url: "https://example.com".to_owned(),
            title: Some("Example".to_owned()),
            provider_metadata: None,
        }),
        Content::ToolResult(ProviderToolResult {
            tool_call_id: "call_2".into(),
            tool_name: "web_search".into(),
            result: json!({ "hits": 3 }),
            is_error: false,
            preliminary: true,
            dynamic: false,
            provider_metadata: None,
        }),
        Content::Custom {
            kind: CustomKind::parse("openai.web_search_call").unwrap(),
            provider_metadata: None,
        },
        Content::ToolApprovalRequest {
            approval_id: "appr_1".into(),
            tool_call_id: "call_3".into(),
            provider_metadata: None,
        },
    ];

    let value = serde_json::to_value(&content).unwrap();
    assert_eq!(
        value,
        json!([
            { "type": "text", "text": "hi" },
            { "type": "tool-call", "tool_call_id": "call_1", "tool_name": "weather",
              "input": "{\"city\":\"Paris\"}" },
            { "type": "source", "source_type": "url", "id": "s1",
              "url": "https://example.com", "title": "Example" },
            { "type": "tool-result", "tool_call_id": "call_2", "tool_name": "web_search",
              "result": { "hits": 3 }, "preliminary": true },
            { "type": "custom", "kind": "openai.web_search_call" },
            { "type": "tool-approval-request", "approval_id": "appr_1", "tool_call_id": "call_3" },
        ])
    );
    let parsed: Vec<Content> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, content);
    assert_eq!(parsed[1].kind_name(), "tool-call");
    assert_eq!(parsed[0].as_text(), Some("hi"));
}

#[test]
fn custom_kind_requires_provider_and_type() {
    let kind = CustomKind::parse("anthropic.server_tool_use").unwrap();
    assert_eq!(kind.provider(), "anthropic");
    assert_eq!(kind.kind(), "server_tool_use");
    assert_eq!(kind.to_string(), "anthropic.server_tool_use");

    for bad in ["", "openai", ".x", "x.", "a b.c", "a.b c"] {
        assert!(
            CustomKind::parse(bad).is_err(),
            "{bad:?} should be rejected"
        );
    }
    assert!(CustomKind::new("a.b", "c").is_err());
    assert!(CustomKind::new("a", "b.c").is_ok());

    let err = serde_json::from_value::<CustomKind>(json!("nodot")).unwrap_err();
    assert!(err.to_string().contains("invalid custom kind"));
}
