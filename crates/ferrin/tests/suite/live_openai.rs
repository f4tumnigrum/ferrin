//! Live tests against the OpenAI API or a compatible endpoint, through the
//! facade (`generate_text`, `stream_text`, tools, structured output).
//!
//! Ignored by default. Run with:
//!
//! ```text
//! OPENAI_API_KEY=... cargo nextest run -p ferrin --all-features --run-ignored only -E 'test(live_)'
//! ```
//!
//! `OPENAI_BASE_URL` selects the endpoint, `OPENAI_MODEL` the model (default
//! `gpt-5`), and `OPENAI_PROVIDER_OPTIONS` passes provider options as JSON
//! (for example `{"openai":{"store":false}}` for endpoints that do not store
//! response items).

use ferrin::openai::OpenAiResponsesLanguageModel;
use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;

fn model() -> OpenAiResponsesLanguageModel {
    assert!(
        env_var("OPENAI_API_KEY").is_some(),
        "set OPENAI_API_KEY to run the live tests"
    );
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());
    create_openai(OpenAiSettings::default())
        .unwrap()
        .responses(&model_id)
}

fn provider_options() -> Option<ProviderOptions> {
    env_var("OPENAI_PROVIDER_OPTIONS")
        .map(|text| serde_json::from_str(&text).expect("OPENAI_PROVIDER_OPTIONS must be JSON"))
}

fn generate<O>(call: GenerateText<O>) -> GenerateText<O> {
    match provider_options() {
        Some(options) => call.provider_options(options),
        None => call,
    }
}

fn stream<O>(call: StreamText<O>) -> StreamText<O> {
    match provider_options() {
        Some(options) => call.provider_options(options),
        None => call,
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct GetWeather {
    /// City name.
    city: String,
}

fn weather_tools() -> ToolSet {
    let tool = Tool::function::<GetWeather>()
        .description("Returns the current weather for a city.")
        .execute(|input: GetWeather, _ctx: ToolContext| async move {
            Ok::<_, ToolError>(json!({ "city": input.city, "temperature_c": 18.5 }))
        })
        .build();
    ToolSet::new().insert("get_weather", tool).unwrap()
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_generate_text_returns_text_and_usage() {
    let result = generate(
        generate_text(model())
            .prompt("Reply with the single word: pong")
            .max_output_tokens(64),
    )
    .await
    .unwrap();
    assert!(!result.text().trim().is_empty(), "{result:?}");
    assert_eq!(result.finish_reason().unified, FinishReasonKind::Stop);
    assert!(result.usage().output.total.is_some_and(|n| n > 0));
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_stream_text_emits_deltas_and_finishes() {
    let result = stream(stream_text(model()).prompt("Count from 1 to 5, separated by spaces."))
        .await
        .unwrap();
    let (mut events, completion) = result.split();
    let mut deltas = 0usize;
    let mut finished = false;
    while let Some(event) = events.next().await {
        match event {
            StreamEvent::TextDelta { .. } => deltas += 1,
            StreamEvent::Finish { .. } => finished = true,
            StreamEvent::Error { error } => panic!("stream error: {error:?}"),
            _ => {}
        }
    }
    let result = completion.await.unwrap();
    assert!(deltas > 0);
    assert!(finished);
    assert!(result.text().contains('5'), "{}", result.text());
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_tool_round_trip_executes_the_tool_and_answers() {
    let result = generate(
        generate_text(model())
            .prompt("What is the weather in Berlin right now? Use the get_weather tool.")
            .tools(weather_tools())
            .stop_when(step_count(3)),
    )
    .await
    .unwrap();
    assert!(result.steps.len() >= 2, "steps: {}", result.steps.len());
    let tool_results = result
        .steps
        .iter()
        .flat_map(StepResult::tool_results)
        .count();
    assert!(tool_results >= 1, "{result:?}");
    assert!(!result.text().trim().is_empty(), "{result:?}");
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Capital {
    city: String,
    country: String,
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_structured_output_deserializes_the_object() {
    let result = generate(
        generate_text(model())
            .prompt("Which city is the capital of France? Answer with the city and the country.")
            .output(Output::<Capital>::object()),
    )
    .await
    .unwrap();
    assert_eq!(result.output.city.to_lowercase(), "paris");
    assert!(result.output.country.to_lowercase().contains("france"));
}
