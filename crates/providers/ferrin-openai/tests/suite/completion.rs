//! Legacy Completions API.

use ferrin_openai::completion::convert_prompt;
use ferrin_spec::CallOptions;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::openai_options;
use super::common::without_raw_usage;

const MODEL: &str = "gpt-3.5-turbo-instruct";

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
        PromptMessage::assistant_text("Hello"),
        PromptMessage::user_text("Again"),
    ]
}

#[test]
fn prompt_converts_to_role_labelled_text() {
    let converted = convert_prompt(&prompt()).unwrap();
    assert_eq!(
        converted.prompt,
        "You are terse.\n\nuser:\nSay hello\n\nassistant:\nHello\n\nuser:\nAgain\n\nassistant:\n"
    );
    assert_eq!(converted.stop_sequences, vec!["\nuser:".to_owned()]);
}

#[test]
fn late_system_message_is_an_invalid_prompt() {
    let error = convert_prompt(&[
        PromptMessage::user_text("Hi"),
        PromptMessage::system("Late"),
    ])
    .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidPrompt(_)),
        "{error:?}"
    );
}

#[test]
fn tool_messages_are_unsupported() {
    let error = convert_prompt(&[PromptMessage::tool(Vec::new())]).unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn generate_maps_text_and_usage() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/completions", "completion", "text-basic");
    let mut options = CallOptions::new(prompt());
    options.stop_sequences = Some(vec!["STOP".to_owned()]);
    options.provider_options =
        openai_options(json!({"echo": false, "logprobs": true, "user": "u1"}));
    let result = test
        .provider
        .completion(MODEL)
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content[0].as_text(), Some("Hello from completion."));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.usage.input.total, Some(8));
    assert_eq!(result.usage.output.total, Some(5));
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stop"], json!(["\nuser:", "STOP"]));
    assert_eq!(request["logprobs"], json!(0));
    assert_eq!(request["echo"], json!(false));
    assert_eq!(request["user"], json!("u1"));
}

#[tokio::test]
async fn tools_and_json_format_produce_warnings() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(prompt());
    options.tools = vec![ToolDefinition::function(
        "t",
        None,
        json!({"type": "object"}),
    )];
    options.tool_choice = Some(ferrin_spec::ToolChoice::Auto);
    options.top_k = Some(3);
    options.response_format = Some(ferrin_spec::ResponseFormat::Json {
        schema: None,
        name: None,
        description: None,
    });
    let prepared = test
        .provider
        .completion(MODEL)
        .prepare_request(&options)
        .unwrap();
    let features: Vec<&str> = prepared
        .warnings
        .iter()
        .filter_map(|warning| match warning {
            ferrin_spec::Warning::Unsupported { feature, .. } => Some(feature.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        features,
        vec!["topK", "tools", "toolChoice", "responseFormat"]
    );
}

#[tokio::test]
async fn stream_emits_single_text_part() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/completions",
        "completion",
        "text-basic-stream",
    );
    let result = test
        .provider
        .completion(MODEL)
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello streamed completion.");
    insta::assert_json_snapshot!("completion_stream_text", parts);
}
