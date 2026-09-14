use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::Headers;
use ferrin_spec::PromptMessage;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::CallOptionsRecord;
use ferrin_spec::language_model::ReasoningEffort;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ToolChoice;

#[test]
fn reasoning_effort_uses_expected_wire_names() {
    let all = [
        (ReasoningEffort::ProviderDefault, "provider-default"),
        (ReasoningEffort::None, "none"),
        (ReasoningEffort::Minimal, "minimal"),
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
    ];
    for (effort, name) in all {
        assert_eq!(serde_json::to_value(effort).unwrap(), json!(name));
        assert_eq!(effort.as_str(), name);
        let parsed: ReasoningEffort = serde_json::from_value(json!(name)).unwrap();
        assert_eq!(parsed, effort);
    }
}

#[test]
fn tool_choice_and_response_format_are_tagged() {
    assert_eq!(
        serde_json::to_value(ToolChoice::tool("weather")).unwrap(),
        json!({ "type": "tool", "tool_name": "weather" })
    );
    assert_eq!(
        serde_json::to_value(ToolChoice::None).unwrap(),
        json!({ "type": "none" })
    );
    assert_eq!(
        serde_json::to_value(ResponseFormat::json(json!({ "type": "object" }))).unwrap(),
        json!({ "type": "json", "schema": { "type": "object" } })
    );
    assert_eq!(
        serde_json::to_value(ResponseFormat::Text).unwrap(),
        json!({ "type": "text" })
    );
}

#[test]
fn recordable_round_trips_and_masks_headers() {
    let mut options = CallOptions::new(vec![PromptMessage::system("be brief")]);
    options.max_output_tokens = Some(10);
    options.headers = Headers::new().with("authorization", "Bearer x");
    options.tool_choice = Some(ToolChoice::Auto);

    let record = options.to_recordable();
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(
        value,
        json!({
            "prompt": [{ "role": "system", "content": "be brief" }],
            "max_output_tokens": 10,
            "tool_choice": { "type": "auto" },
            "reasoning": "provider-default",
            "headers": { "authorization": "***" },
        })
    );

    let parsed: CallOptionsRecord = serde_json::from_value(json!({
        "prompt": [{ "role": "system", "content": "be brief" }],
        "max_output_tokens": 10,
        "tool_choice": { "type": "auto" },
    }))
    .unwrap();
    let restored = CallOptions::from(parsed);
    assert_eq!(restored.max_output_tokens, Some(10));
    assert_eq!(restored.prompt, options.prompt);
    assert!(restored.headers.is_empty());
}
