//! Live tests against the OpenAI API or a compatible endpoint (Responses and
//! Chat Completions).
//!
//! Ignored by default. Run with:
//!
//! ```text
//! OPENAI_API_KEY=... cargo nextest run -p ferrin-openai --all-features --run-ignored only -E 'test(live_)'
//! ```
//!
//! `OPENAI_BASE_URL` selects the endpoint, `OPENAI_MODEL` the model (default
//! `gpt-5`) and `OPENAI_PROVIDER_OPTIONS` passes provider options as JSON.

use ferrin_openai::OpenAiProvider;
use ferrin_openai::OpenAiSettings;
use ferrin_openai::create_openai;
use ferrin_provider_util::settings::env_var;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderOptions;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use futures_util::StreamExt;
use serde_json::json;

fn provider() -> (OpenAiProvider, String) {
    assert!(
        env_var("OPENAI_API_KEY").is_some(),
        "set OPENAI_API_KEY to run the live tests"
    );
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());
    (create_openai(OpenAiSettings::default()).unwrap(), model_id)
}

fn call_options(prompt: Vec<PromptMessage>) -> CallOptions {
    let mut options = CallOptions::new(prompt);
    if let Some(text) = env_var("OPENAI_PROVIDER_OPTIONS") {
        options.provider_options =
            serde_json::from_str::<ProviderOptions>(&text).expect("OPENAI_PROVIDER_OPTIONS");
    }
    options
}

fn weather_tool() -> ToolDefinition {
    ToolDefinition::function(
        "get_weather",
        Some("Returns the current weather for a city.".to_owned()),
        json!({
            "type": "object",
            "properties": {"city": {"type": "string", "description": "City name."}},
            "required": ["city"],
            "additionalProperties": false
        }),
    )
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_responses_generate_returns_text() {
    let (openai, model_id) = provider();
    let model = openai.responses(&model_id);
    let result = model
        .do_generate(call_options(vec![PromptMessage::user_text(
            "Reply with the single word: pong",
        )]))
        .await
        .unwrap();
    let text = result
        .content
        .iter()
        .filter_map(Content::as_text)
        .collect::<String>();
    assert!(!text.trim().is_empty(), "{result:?}");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert!(result.usage.output.total.is_some_and(|n| n > 0));
    assert!(result.response.id.is_some());
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_responses_stream_emits_text_deltas_and_finish() {
    let (openai, model_id) = provider();
    let model = openai.responses(&model_id);
    let result = model
        .do_stream(call_options(vec![PromptMessage::user_text(
            "Count from 1 to 5, separated by spaces.",
        )]))
        .await
        .unwrap();
    let parts = result.stream.collect::<Vec<_>>().await;
    let text = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains('5'), "{text}");
    assert!(matches!(
        parts.first(),
        Some(StreamPart::StreamStart { .. })
    ));
    assert!(matches!(
        parts.last(),
        Some(StreamPart::Finish { finish_reason, .. })
            if finish_reason.unified == FinishReasonKind::Stop
    ));
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_responses_generate_emits_a_tool_call() {
    let (openai, model_id) = provider();
    let model = openai.responses(&model_id);
    let mut options = call_options(vec![PromptMessage::user_text(
        "What is the weather in Berlin? Use the get_weather tool.",
    )]);
    options.tools = vec![weather_tool()];
    let result = model.do_generate(options).await.unwrap();
    let call = result
        .content
        .iter()
        .find_map(Content::as_tool_call)
        .unwrap_or_else(|| panic!("no tool call in {:?}", result.content));
    assert_eq!(call.tool_name.as_str(), "get_weather");
    let input: JsonValue = serde_json::from_str(&call.input).unwrap();
    assert!(input["city"].is_string(), "{input}");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY"]
async fn live_chat_generate_returns_text() {
    let (openai, model_id) = provider();
    let model = openai.chat(&model_id);
    let result = model
        .do_generate(call_options(vec![PromptMessage::user_text(
            "Reply with the single word: pong",
        )]))
        .await
        .unwrap();
    let text = result
        .content
        .iter()
        .filter_map(Content::as_text)
        .collect::<String>();
    assert!(!text.trim().is_empty(), "{result:?}");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
}
