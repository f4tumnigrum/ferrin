//! Shared fixtures for the core test suite.

use std::sync::Arc;

use ferrin_core::StepContent;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::language_model::GenerateResult;
use ferrin_testing::MockLanguageModel;
use ferrin_testing::MockLanguageModelBuilder;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use serde_json::json;

/// A generate result with one text part and `stop`.
pub(crate) fn text_result(text: &str) -> GenerateResult {
    let mut result = GenerateResult::new(vec![Content::text(text)], FinishReason::stop());
    result.usage = Usage::totals(10, 5);
    result
}

/// A generate result with one tool call and `tool-calls`.
pub(crate) fn tool_call_result(id: &str, name: &str, input: &JsonValue) -> GenerateResult {
    let mut result = GenerateResult::new(
        vec![Content::ToolCall(ToolCall::new(
            id,
            name,
            input.to_string(),
        ))],
        FinishReason::tool_calls(),
    );
    result.usage = Usage::totals(20, 8);
    result
}

/// A mock that answers `text` to every generate call.
pub(crate) fn text_model(text: &str) -> Arc<MockLanguageModel> {
    MockLanguageModel::builder()
        .generate_repeat(text_result(text))
        .build_shared()
}

/// A mock builder with the default identity used by the tests.
pub(crate) fn mock() -> MockLanguageModelBuilder {
    MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
}

/// The `get_weather` tool: `{ city: string }` in, `{ city, temperature }` out.
pub(crate) fn weather_tool() -> Tool {
    Tool::function_with_schema(Schema::from_json_schema(json!({
        "type": "object",
        "properties": { "city": { "type": "string" } },
        "required": ["city"],
        "additionalProperties": false
    })))
    .description("Get the weather for a city.")
    .execute(|input: JsonValue, _ctx: ToolContext| async move {
        let city = input["city"].as_str().unwrap_or_default().to_owned();
        Ok::<_, ToolError>(json!({ "city": city, "temperature": 21 }))
    })
    .build()
}

/// A tool set containing only `get_weather`.
pub(crate) fn weather_tools() -> ToolSet {
    ToolSet::new()
        .insert("get_weather", weather_tool())
        .unwrap()
}

/// Kind names of the step content, in order.
pub(crate) fn kinds(content: &[StepContent]) -> Vec<&'static str> {
    content.iter().map(StepContent::kind_name).collect()
}
