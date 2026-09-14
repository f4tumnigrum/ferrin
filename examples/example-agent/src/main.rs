//! A reusable `ToolLoopAgent` with tools defined by `#[ferrin::tool]`.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-agent
//! ```
//!
//! `OPENAI_BASE_URL` and `OPENAI_MODEL` (default `gpt-5`) select the endpoint
//! and model; `OPENAI_PROVIDER_OPTIONS` passes provider options as JSON.

#![allow(clippy::print_stdout)]

use std::sync::Arc;

use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;

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
    condition: &'static str,
}

/// Returns the current weather for a city.
#[ferrin::tool]
async fn get_weather(input: GetWeather) -> Result<Weather, ToolError> {
    // Replace with a real weather API call.
    let temperature_c = match input.city.to_lowercase().as_str() {
        "berlin" => 18.5,
        "tokyo" => 27.0,
        _ => 21.0,
    };
    Ok(Weather {
        city: input.city,
        temperature_c,
        condition: "partly cloudy",
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Convert {
    /// Temperature in Celsius.
    celsius: f32,
}

/// Converts Celsius to Fahrenheit.
#[ferrin::tool]
fn celsius_to_fahrenheit(input: Convert) -> Result<f32, ToolError> {
    Ok(input.celsius * 9.0 / 5.0 + 32.0)
}

/// Extra provider options from `OPENAI_PROVIDER_OPTIONS`, a JSON object keyed
/// by provider name. Example for endpoints that do not store response items:
/// `{"openai":{"store":false}}`.
fn provider_options() -> anyhow::Result<Option<ProviderOptions>> {
    env_var("OPENAI_PROVIDER_OPTIONS")
        .map(|text| Ok(ferrin::serde_json::from_str(&text)?))
        .transpose()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());

    let tools = ToolSet::new()
        .insert("get_weather", get_weather())?
        .insert("celsius_to_fahrenheit", celsius_to_fahrenheit())?;

    let mut builder = ToolLoopAgent::builder(openai.responses(&model_id));
    if let Some(options) = provider_options()? {
        builder = builder.provider_options(options);
    }
    let agent = builder
        .id("weather-assistant")
        .instructions("You answer weather questions. Use the tools; report both °C and °F.")
        .tools(tools)
        .stop_when(step_count(6))
        .on_step_end(|step: Arc<StepResult>| async move {
            for call in step.tool_calls() {
                println!("-> tool call {}({})", call.tool_name, call.input);
            }
        })
        .build();

    let result = agent
        .generate(AgentCall::prompt(
            "What is the weather in Berlin and Tokyo right now?",
        ))
        .await?;

    println!();
    println!("{}", result.text());
    println!(
        "({} steps, {} output tokens)",
        result.steps.len(),
        result.total_usage.output.total.unwrap_or(0)
    );
    Ok(())
}
