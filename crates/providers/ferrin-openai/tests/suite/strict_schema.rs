//! Reference strict flags leave application schema constraints unchanged.

use ferrin_spec::CallOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[tokio::test]
async fn strict_flags_preserve_optional_values_and_dictionary_schemas() {
    let test = TestProvider::start().await;
    for schema in [
        json!({"type":"object","properties":{"mode":{"type":"string","enum":["fast","slow"]},"nested":{"type":"object","properties":{"enabled":{"type":"boolean"}}}},"required":[]}),
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
                call.provider_options = openai_options(json!({"strictJsonSchema":global}));
                let responses = ferrin_openai::responses::request::prepare_request(
                    test.provider.config(),
                    "gpt-4.1",
                    &call,
                )
                .unwrap();
                let chat = test
                    .provider
                    .chat("gpt-4.1")
                    .prepare_request(&call)
                    .unwrap();
                let responses = serde_json::to_value(responses.body).unwrap();
                let chat = serde_json::to_value(chat.body).unwrap();
                assert_eq!(
                    [
                        responses["tools"][0]["parameters"].clone(),
                        responses["text"]["format"]["schema"].clone(),
                        chat["tools"][0]["function"]["parameters"].clone(),
                        chat["response_format"]["json_schema"]["schema"].clone()
                    ],
                    std::array::from_fn::<_, 4, _>(|_| schema.clone()),
                );
                assert_eq!(
                    (
                        responses["tools"][0].get("strict"),
                        chat["tools"][0]["function"].get("strict")
                    ),
                    (
                        local.map(JsonValue::Bool).as_ref(),
                        local.map(JsonValue::Bool).as_ref()
                    ),
                );
                assert_eq!(
                    (
                        responses["text"]["format"]["strict"].clone(),
                        chat["response_format"]["json_schema"]["strict"].clone()
                    ),
                    (json!(global), json!(global)),
                );
            }
        }
    }
}
