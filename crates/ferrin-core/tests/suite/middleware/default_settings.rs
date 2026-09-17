use ferrin_core::middleware::builtin::CallDefaults;
use ferrin_core::middleware::builtin::default_settings;
use ferrin_core::middleware::builtin::merge_json_objects;
use ferrin_spec::Headers;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::options;

fn object(value: serde_json::Value) -> ferrin_spec::JsonObject {
    value.as_object().unwrap().clone()
}

#[test]
fn fills_unset_scalars_and_keeps_explicit_values() {
    let middleware = default_settings(CallDefaults {
        temperature: Some(0.2),
        max_output_tokens: Some(100),
        seed: Some(7),
        tool_choice: Some(ToolChoice::Auto),
        ..CallDefaults::default()
    });
    let mut call = options();
    call.temperature = Some(0.9);
    let result = middleware.apply(call);
    assert_eq!(result.temperature, Some(0.9));
    assert_eq!(result.max_output_tokens, Some(100));
    assert_eq!(result.seed, Some(7));
    assert_eq!(result.tool_choice, Some(ToolChoice::Auto));
    assert_eq!(result.top_p, None);
}

#[test]
fn tools_apply_only_when_the_call_sends_none() {
    let default_tool = ToolDefinition::Function {
        name: "default_tool".into(),
        description: None,
        input_schema: json!({ "type": "object" }),
        strict: None,
        input_examples: Vec::new(),
        provider_options: None,
    };
    let middleware = default_settings(CallDefaults {
        tools: vec![default_tool.clone()],
        ..CallDefaults::default()
    });
    assert_eq!(middleware.apply(options()).tools, vec![default_tool]);

    let call_tool = ToolDefinition::Function {
        name: "call_tool".into(),
        description: None,
        input_schema: json!({ "type": "object" }),
        strict: None,
        input_examples: Vec::new(),
        provider_options: None,
    };
    let mut call = options();
    call.tools = vec![call_tool.clone()];
    assert_eq!(middleware.apply(call).tools, vec![call_tool]);
}

#[test]
fn headers_and_provider_options_merge_with_call_precedence() {
    let middleware = default_settings(CallDefaults {
        headers: Headers::new()
            .with("x-default", "1")
            .with("x-shared", "default"),
        provider_options: [(
            "openai".to_owned(),
            object(json!({ "reasoning": { "effort": "low", "summary": "auto" }, "store": true })),
        )]
        .into_iter()
        .collect(),
        ..CallDefaults::default()
    });
    let mut call = options();
    call.headers = Headers::new().with("x-shared", "call").with("x-call", "2");
    call.provider_options = [
        (
            "openai".to_owned(),
            object(json!({ "reasoning": { "effort": "high" }, "user": "u1" })),
        ),
        ("anthropic".to_owned(), object(json!({ "x": 1 }))),
    ]
    .into_iter()
    .collect();
    let result = middleware.apply(call);
    assert_eq!(result.headers.get_str("x-default"), Some("1"));
    assert_eq!(result.headers.get_str("x-shared"), Some("call"));
    assert_eq!(result.headers.get_str("x-call"), Some("2"));
    assert_eq!(
        result.provider_options["openai"],
        object(json!({
            "reasoning": { "effort": "high", "summary": "auto" },
            "store": true,
            "user": "u1"
        }))
    );
    assert_eq!(
        result.provider_options["anthropic"],
        object(json!({ "x": 1 }))
    );
}

#[test]
fn merge_json_objects_overrides_arrays_and_scalars() {
    let merged = merge_json_objects(
        &object(json!({ "a": [1, 2], "b": { "c": 1, "d": 2 }, "e": 1 })),
        object(json!({ "a": [3], "b": { "c": null }, "e": { "nested": true } })),
    );
    assert_eq!(
        merged,
        object(json!({ "a": [3], "b": { "c": null, "d": 2 }, "e": { "nested": true } }))
    );
}

#[test]
fn reserved_override_keys_are_excluded_at_each_merge_level() {
    let merged = merge_json_objects(
        &object(json!({"nested":{"keep":true},"constructor":"base"})),
        object(
            json!({"__proto__":{"polluted":true},"prototype":1,"constructor":2,
            "nested":{"__proto__":1,"constructor":2,"prototype":3,"safe":4}}),
        ),
    );
    assert_eq!(
        merged,
        object(json!({"nested":{"keep":true,"safe":4},"constructor":"base"}))
    );
}

#[test]
fn json_response_format_merges_nested_defaults_with_call_precedence() {
    use ferrin_spec::ResponseFormat;
    let middleware = default_settings(CallDefaults {
        response_format: Some(ResponseFormat::Json {
            schema: Some(
                json!({"type":"object","properties":{"default":{"type":"string"}},"required":["default"]}),
            ),
            name: Some("default-name".into()),
            description: Some("default-description".into()),
        }),
        ..Default::default()
    });
    let mut call = options();
    call.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({"properties":{"call":{"type":"number"}},"required":[]})),
        name: Some("call-name".into()),
        description: None,
    });
    assert_eq!(
        middleware.apply(call).response_format,
        Some(ResponseFormat::Json {
            schema: Some(
                json!({"type":"object","properties":{"default":{"type":"string"},"call":{"type":"number"}},"required":[]})
            ),
            name: Some("call-name".into()),
            description: Some("default-description".into()),
        })
    );
    let mut call = options();
    call.response_format = Some(ResponseFormat::json_unconstrained());
    let defaults = middleware.apply(call).response_format.unwrap();
    assert!(matches!(
        defaults,
        ResponseFormat::Json {
            schema: Some(_),
            name: Some(_),
            description: Some(_)
        }
    ));
    let mut call = options();
    call.response_format = Some(ResponseFormat::Text);
    assert_eq!(
        middleware.apply(call).response_format,
        Some(ResponseFormat::Text)
    );
}
