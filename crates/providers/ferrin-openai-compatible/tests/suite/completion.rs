//! Legacy Completions.

use ferrin_openai_compatible::completion::convert_prompt;
use ferrin_spec::CallOptions;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::example_options;
use super::common::without_raw_usage;

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
        PromptMessage::assistant_text("Hello."),
        PromptMessage::user_text("Again"),
    ]
}

#[tokio::test]
async fn generate_converts_the_prompt_and_maps_text() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/completions", "completion", "text-basic");
    let mut options = CallOptions::new(prompt());
    options.stop_sequences = Some(vec!["STOP".to_owned()]);
    options.max_output_tokens = Some(16);
    let result = test
        .provider
        .completion("example-completion")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.content[0].as_text(), Some("Hello from completion."));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.usage.input.total, Some(8));
    assert_eq!(result.usage.output.total, Some(5));
    assert_eq!(result.response.id.as_deref(), Some("cmpl_1"));
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "example-completion",
            "max_tokens": 16,
            "prompt": "You are terse.\n\nuser:\nSay hello\n\nassistant:\nHello.\n\nuser:\nAgain\n\nassistant:\n",
            "stop": ["\nuser:", "STOP"]
        })
    );
}

#[tokio::test]
async fn stream_emits_text_and_finish() {
    let test = TestProvider::start_with(|mut settings| {
        settings.include_usage = true;
        settings
    })
    .await;
    test.mount(
        Method::POST,
        "/v1/completions",
        "completion",
        "text-basic-stream",
    );
    let result = test
        .provider
        .completion("example-completion")
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap();
    let parts = without_raw_usage(collect_checked(result).await);
    insta::assert_json_snapshot!("completion_stream_text", parts);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["stream"], json!(true));
    assert_eq!(request["stream_options"], json!({"include_usage": true}));
}

#[tokio::test]
async fn options_are_mapped_and_unknown_keys_pass_through() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(prompt());
    options.top_k = Some(2);
    options.tools = vec![ToolDefinition::function("t", None, json!({}))];
    options.tool_choice = Some(ToolChoice::Auto);
    options.response_format = Some(ResponseFormat::Json {
        schema: None,
        name: None,
        description: None,
    });
    options.provider_options = example_options(json!({
        "echo": true,
        "logitBias": {"50256": -100.0},
        "suffix": "!",
        "user": "u1",
        "bestOf": 2
    }));
    let prepared = test
        .provider
        .completion("example-completion")
        .prepare_request(&options)
        .unwrap();
    assert_eq!(
        prepared.warnings,
        vec![
            Warning::unsupported("topK"),
            Warning::unsupported("tools"),
            Warning::unsupported("toolChoice"),
            Warning::unsupported_with_details(
                "responseFormat",
                "JSON response format is not supported."
            ),
        ]
    );
    let body = serde_json::Value::Object(prepared.body);
    assert_eq!(body["echo"], json!(true));
    assert_eq!(body["logit_bias"], json!({"50256": -100.0}));
    assert_eq!(body["suffix"], json!("!"));
    assert_eq!(body["user"], json!("u1"));
    assert_eq!(body["bestOf"], json!(2));
    assert!(body.get("tools").is_none());
}

#[test]
fn prompt_conversion_rejects_late_system_messages_and_tools() {
    let late_system = vec![
        PromptMessage::user_text("hi"),
        PromptMessage::system("late"),
    ];
    let error = convert_prompt(&late_system).unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidPrompt(_)),
        "{error:?}"
    );

    let tool_call = vec![PromptMessage::assistant(vec![
        AssistantPromptPart::ToolCall(ToolCallPart {
            tool_call_id: "c".into(),
            tool_name: "t".into(),
            input: json!({}),
            provider_executed: false,
            provider_options: None,
        }),
    ])];
    let error = convert_prompt(&tool_call).unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );

    let tool_message = vec![PromptMessage::tool(vec![])];
    let error = convert_prompt(&tool_message).unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
}
