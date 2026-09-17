//! Compatible endpoints receive original schemas with independent strict flags.

use ferrin_spec::CallOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::example_options;

#[tokio::test]
async fn strict_flags_preserve_optional_values_and_dictionary_schemas() {
    let test = TestProvider::start_with(|mut settings| {
        settings.supports_structured_outputs = true;
        settings
    })
    .await;
    for schema in [
        json!({"type":"object","properties":{"mode":{"type":"string","enum":["fast","slow"]}}}),
        json!({"type":"object","additionalProperties":{"type":"integer"}}),
    ] {
        for global in [true, false] {
            for local in [None, Some(false), Some(true)] {
                let mut call = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
                let mut tool = ToolDefinition::function("lookup", None, schema.clone());
                let ToolDefinition::Function { strict, .. } = &mut tool else {
                    panic!("expected function")
                };
                *strict = local;
                call.tools.push(tool);
                call.response_format = Some(ResponseFormat::json(schema.clone()));
                call.provider_options = example_options(json!({"strictJsonSchema":global}));
                let body = test
                    .provider
                    .chat("model")
                    .prepare_request(&call)
                    .unwrap()
                    .body;
                assert_eq!(
                    (
                        body["tools"][0]["function"]["parameters"].clone(),
                        body["response_format"]["json_schema"].clone()
                    ),
                    (
                        schema.clone(),
                        json!({"schema":schema,"strict":global,"name":"response"})
                    ),
                );
                assert_eq!(
                    body["tools"][0]["function"].get("strict"),
                    local.map(JsonValue::Bool).as_ref()
                );
            }
        }
    }
}

#[tokio::test]
async fn unsupported_structured_output_retains_json_object_fallback() {
    let test = TestProvider::start().await;
    let mut call = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    call.response_format = Some(ResponseFormat::json(
        json!({"type":"object","additionalProperties":{"type":"integer"}}),
    ));
    let request = test.provider.chat("model").prepare_request(&call).unwrap();
    assert_eq!(
        request.body["response_format"],
        json!({"type":"json_object"})
    );
    assert_eq!(request.warnings.len(), 1);
}
