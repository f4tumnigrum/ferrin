//! Strict schema conversion respects compatible endpoint opt-outs.

use ferrin_spec::CallOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::example_options;

fn options(schema: &JsonValue) -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.tools = vec![ToolDefinition::function("lookup", None, schema.clone())];
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(schema.clone()),
        name: None,
        description: None,
    });
    options
}

#[tokio::test]
async fn structured_output_and_explicit_tool_strict_transform_optional_values() {
    let test = TestProvider::start_with(|mut settings| {
        settings.supports_structured_outputs = true;
        settings
    })
    .await;
    let schema = json!({"type": "object", "properties": {
        "mode": {"type": "string", "enum": ["fast", "slow"]}
    }});
    let expected = json!({"type": "object", "properties": {
        "mode": {"anyOf": [{"type": "string", "enum": ["fast", "slow"]}, {"type": "null"}]}
    }, "required": ["mode"], "additionalProperties": false});
    for strict in [None, Some(false), Some(true)] {
        let mut options = options(&schema);
        let ToolDefinition::Function { strict: value, .. } = &mut options.tools[0] else {
            panic!("expected function")
        };
        *value = strict;
        let body = test
            .provider
            .chat("example-model")
            .prepare_request(&options)
            .unwrap()
            .body;
        assert_eq!(body["response_format"]["json_schema"]["schema"], expected);
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            if strict == Some(true) {
                expected.clone()
            } else {
                schema.clone()
            }
        );
        assert_eq!(
            body["tools"][0]["function"].get("strict"),
            strict.map(JsonValue::Bool).as_ref()
        );
        options.provider_options = example_options(json!({"strictJsonSchema": false}));
        let body = test
            .provider
            .chat("example-model")
            .prepare_request(&options)
            .unwrap()
            .body;
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(
            body["response_format"]["json_schema"]["strict"],
            json!(false)
        );
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            if strict == Some(true) {
                expected.clone()
            } else {
                schema.clone()
            }
        );
    }
}

#[tokio::test]
async fn strict_dictionaries_fail_but_opt_out_and_schema_fallback_remain_available() {
    let schema = json!({"type": "object", "additionalProperties": {"type": "integer"}});
    let test = TestProvider::start_with(|mut settings| {
        settings.supports_structured_outputs = true;
        settings
    })
    .await;
    let mut options = options(&schema);
    assert!(matches!(
        test.provider
            .chat("example-model")
            .prepare_request(&options),
        Err(ProviderError::InvalidArgument(_))
    ));
    options.provider_options = example_options(json!({"strictJsonSchema": false}));
    let body = test
        .provider
        .chat("example-model")
        .prepare_request(&options)
        .unwrap()
        .body;
    assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
    let ToolDefinition::Function { strict, .. } = &mut options.tools[0] else {
        panic!("expected function")
    };
    *strict = Some(true);
    assert!(matches!(
        test.provider
            .chat("example-model")
            .prepare_request(&options),
        Err(ProviderError::InvalidArgument(_))
    ));

    let fallback = TestProvider::start().await;
    options.tools.clear();
    options.provider_options.clear();
    let body = fallback
        .provider
        .chat("example-model")
        .prepare_request(&options)
        .unwrap()
        .body;
    assert_eq!(body["response_format"], json!({"type": "json_object"}));
}
