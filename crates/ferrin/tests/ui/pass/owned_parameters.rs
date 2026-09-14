use ferrin::tool::ToolContext;
use ferrin::tool::ToolError;

#[derive(ferrin::serde::Deserialize, ferrin::schemars::JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Input {
    city: String,
}

/// Get the weather.
#[ferrin::tool]
pub async fn get_weather(input: Input) -> Result<String, ToolError> {
    Ok(input.city)
}

/// Echo the input with the tool call id.
#[ferrin::tool]
fn echo(input: Input, ctx: ToolContext) -> Result<String, ToolError> {
    Ok(format!("{}:{}", input.city, ctx.tool_call_id.as_str()))
}

fn main() {
    let _weather = get_weather();
    let _echo = echo();
}
