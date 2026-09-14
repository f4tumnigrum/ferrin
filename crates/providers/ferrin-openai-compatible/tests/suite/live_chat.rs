//! Live tests against an OpenAI-compatible endpoint.
//!
//! Ignored by default. Run with:
//!
//! ```text
//! OPENAI_COMPATIBLE_BASE_URL=https://host/v1 OPENAI_COMPATIBLE_API_KEY=... \
//!   OPENAI_COMPATIBLE_MODEL=... cargo nextest run -p ferrin-openai-compatible \
//!   --all-features --run-ignored only -E 'test(live_)'
//! ```

use ferrin_openai_compatible::OpenAiCompatibleProvider;
use ferrin_openai_compatible::OpenAiCompatibleSettings;
use ferrin_openai_compatible::create_openai_compatible;
use ferrin_provider_util::settings::env_var;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use url::Url;

use super::common::collect_checked;

fn provider() -> (OpenAiCompatibleProvider, String) {
    let base_url = env_var("OPENAI_COMPATIBLE_BASE_URL")
        .expect("set OPENAI_COMPATIBLE_BASE_URL to run the live tests");
    let model_id = env_var("OPENAI_COMPATIBLE_MODEL")
        .expect("set OPENAI_COMPATIBLE_MODEL to run the live tests");
    let mut settings = OpenAiCompatibleSettings::new("live", Url::parse(&base_url).unwrap());
    settings.api_key_env = Some("OPENAI_COMPATIBLE_API_KEY".to_owned());
    (create_openai_compatible(settings).unwrap(), model_id)
}

fn options(text: &str) -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text(text)]);
    options.max_output_tokens = Some(64);
    options
}

#[tokio::test]
#[ignore = "needs OPENAI_COMPATIBLE_* environment variables"]
async fn live_chat_generate_returns_text_and_usage() {
    let (provider, model_id) = provider();
    let result = provider
        .chat(&model_id)
        .do_generate(options("Reply with the single word: pong"))
        .await
        .unwrap();
    let text = result
        .content
        .iter()
        .filter_map(Content::as_text)
        .collect::<String>();
    assert!(!text.trim().is_empty(), "{result:?}");
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert!(result.usage.input.total.is_some_and(|n| n > 0));
}

#[tokio::test]
#[ignore = "needs OPENAI_COMPATIBLE_* environment variables"]
async fn live_chat_stream_satisfies_the_stream_contract() {
    let (provider, model_id) = provider();
    let result = provider
        .chat(&model_id)
        .do_stream(options("Count from 1 to 5, separated by spaces."))
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    let text = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains('5'), "{text}");
    assert!(matches!(
        parts.last(),
        Some(StreamPart::Finish { finish_reason, .. })
            if finish_reason.unified == FinishReasonKind::Stop
    ));
}
