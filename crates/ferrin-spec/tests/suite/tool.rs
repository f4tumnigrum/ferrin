use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::ToolDefinition;

#[test]
fn tool_definitions_round_trip() {
    let tools = vec![
        ToolDefinition::function(
            "weather",
            Some("Get weather".to_owned()),
            json!({ "type": "object", "properties": { "city": { "type": "string" } } }),
        ),
        ToolDefinition::provider(
            "openai.web_search",
            "web_search",
            json!({ "search_context_size": "low" })
                .as_object()
                .unwrap()
                .clone(),
        ),
    ];
    let value = serde_json::to_value(&tools).unwrap();
    assert_eq!(
        value,
        json!([
            { "type": "function", "name": "weather", "description": "Get weather",
              "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } } },
            { "type": "provider", "id": "openai.web_search", "name": "web_search",
              "args": { "search_context_size": "low" } },
        ])
    );
    let parsed: Vec<ToolDefinition> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, tools);
    assert_eq!(parsed[0].name().as_str(), "weather");
    assert!(parsed[1].is_provider_tool());
}
