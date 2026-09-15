//! Effective strict mode controls every tool and structured-output schema.

use ferrin_spec::CallOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

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

fn schema_locations(test: &TestProvider, options: &CallOptions) -> [JsonValue; 4] {
    let responses = ferrin_openai::responses::request::prepare_request(
        test.provider.config(),
        "gpt-4.1",
        options,
    )
    .unwrap();
    let chat = test
        .provider
        .chat("gpt-4.1")
        .prepare_request(options)
        .unwrap();
    let responses = serde_json::to_value(responses.body).unwrap();
    let chat = serde_json::to_value(chat.body).unwrap();
    [
        responses["tools"][0]["parameters"].clone(),
        responses["text"]["format"]["schema"].clone(),
        chat["tools"][0]["function"]["parameters"].clone(),
        chat["response_format"]["json_schema"]["schema"].clone(),
    ]
}

#[tokio::test]
async fn default_strict_transforms_all_four_nested_optional_schemas() {
    let test = TestProvider::start().await;
    let schema = json!({"type": "object", "properties": {
        "mode": {"type": "string", "enum": ["fast", "slow"]},
        "nested": {"type": "object", "properties": {"enabled": {"type": "boolean"}}}
    }, "required": []});
    let expected = json!({"type": "object", "properties": {
        "mode": {"anyOf": [{"type": "string", "enum": ["fast", "slow"]}, {"type": "null"}]},
        "nested": {"anyOf": [{"type": "object", "properties": {
            "enabled": {"anyOf": [{"type": "boolean"}, {"type": "null"}]}
        }, "required": ["enabled"], "additionalProperties": false}, {"type": "null"}]}
    }, "required": ["mode", "nested"], "additionalProperties": false});
    assert_eq!(
        schema_locations(&test, &options(&schema)),
        std::array::from_fn(|_| expected.clone())
    );
    let mut relaxed = options(&schema);
    relaxed.provider_options = openai_options(json!({"strictJsonSchema": false}));
    assert_eq!(
        schema_locations(&test, &relaxed),
        std::array::from_fn(|_| schema.clone())
    );
}

#[tokio::test]
async fn function_strict_override_controls_conversion_independently() {
    let test = TestProvider::start().await;
    let schema = json!({"type": "object", "properties": {"value": {"type": "string"}}});
    for (global, local) in [(true, false), (false, true)] {
        let mut options = options(&schema);
        options.provider_options = openai_options(json!({"strictJsonSchema": global}));
        let ToolDefinition::Function { strict, .. } = &mut options.tools[0] else {
            panic!("expected function")
        };
        *strict = Some(local);
        let actual = schema_locations(&test, &options);
        let strict_schema = json!({"type": "object", "properties": {"value": {
            "anyOf": [{"type": "string"}, {"type": "null"}]
        }}, "required": ["value"], "additionalProperties": false});
        let tool_expected = if local { &strict_schema } else { &schema };
        let output_expected = if global { &strict_schema } else { &schema };
        assert_eq!(
            actual,
            [
                tool_expected.clone(),
                output_expected.clone(),
                tool_expected.clone(),
                output_expected.clone()
            ]
        );
    }
}

#[tokio::test]
async fn strict_rejects_dictionaries_at_every_request_position() {
    let test = TestProvider::start().await;
    for tool_schema in [true, false] {
        let schema = json!({"type": "object", "additionalProperties": {"type": "integer"}});
        let mut options = options(&schema);
        if tool_schema {
            options.response_format = None;
        } else {
            options.tools.clear();
        }
        assert!(matches!(
            ferrin_openai::responses::request::prepare_request(
                test.provider.config(),
                "gpt-4.1",
                &options
            ),
            Err(ProviderError::InvalidArgument(_))
        ));
        assert!(matches!(
            test.provider.chat("gpt-4.1").prepare_request(&options),
            Err(ProviderError::InvalidArgument(_))
        ));
        options.provider_options = openai_options(json!({"strictJsonSchema": false}));
        assert!(
            ferrin_openai::responses::request::prepare_request(
                test.provider.config(),
                "gpt-4.1",
                &options
            )
            .is_ok()
        );
        assert!(
            test.provider
                .chat("gpt-4.1")
                .prepare_request(&options)
                .is_ok()
        );
    }
}
