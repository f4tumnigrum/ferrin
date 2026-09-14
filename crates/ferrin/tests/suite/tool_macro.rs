//! `#[ferrin::tool]` expansion.

use ferrin::prelude::*;
use ferrin::tool::Description;
use ferrin::tool::ToolKind;
use ferrin::tool::execute_to_completion;
use pretty_assertions::assert_eq;

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct GetWeather {
    /// City name.
    city: String,
}

#[derive(Serialize)]
#[serde(crate = "ferrin::serde")]
struct Weather {
    city: String,
    temperature_c: f32,
}

/// Get the current weather for a city.
///
/// Temperatures are in Celsius.
#[ferrin::tool]
pub async fn get_weather(input: GetWeather) -> Result<Weather, ToolError> {
    Ok(Weather {
        city: input.city,
        temperature_c: 21.5,
    })
}

#[tool]
fn echo(input: GetWeather, ctx: ToolContext) -> Result<JsonValue, ToolError> {
    Ok(json!({ "city": input.city, "call": ctx.tool_call_id.as_str() }))
}

#[tokio::test]
async fn an_async_function_becomes_a_function_tool_described_by_its_doc_comment() {
    let tool = get_weather();
    assert_eq!(tool.kind(), &ToolKind::Function);
    assert_eq!(
        tool.description().and_then(Description::as_static),
        Some("Get the current weather for a city.\n\nTemperatures are in Celsius.")
    );
    let schema = tool.input_schema().json_schema();
    assert_eq!(schema["properties"]["city"]["type"], json!("string"));
    assert_eq!(
        schema["properties"]["city"]["description"],
        json!("City name.")
    );
    let stream = tool
        .execute(json!({ "city": "Berlin" }), ToolContext::new("tc-1"))
        .unwrap();
    let output = execute_to_completion(stream, |_| {}).await.unwrap();
    assert_eq!(output, json!({ "city": "Berlin", "temperature_c": 21.5 }));
}

#[tokio::test]
async fn a_sync_function_with_a_context_parameter_receives_the_context() {
    let tool = echo();
    assert!(tool.description().is_none());
    let stream = tool
        .execute(json!({ "city": "Oslo" }), ToolContext::new("tc-9"))
        .unwrap();
    let output = execute_to_completion(stream, |_| {}).await.unwrap();
    assert_eq!(output, json!({ "city": "Oslo", "call": "tc-9" }));
}
