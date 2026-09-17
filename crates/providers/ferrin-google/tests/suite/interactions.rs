//! Interactions wire-schema fixture regressions; no live API compatibility claim.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::google_options;

fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text("Hello")])
}

#[tokio::test]
async fn request_preserves_json_schema_and_maps_options() {
    let test = TestProvider::start().await;
    let mut call = options();
    call.prompt.insert(0, PromptMessage::system("Be precise"));
    call.tools.push(ToolDefinition::function(
        "weather",
        None,
        json!({"type":"object", "properties":{"camelCase":{"type":"string"}}}),
    ));
    call.tool_choice = Some(ToolChoice::Required);
    call.response_format = Some(ResponseFormat::json(
        json!({"type":"object","properties":{"camelCase":{"type":"string"}}}),
    ));
    call.provider_options = google_options(
        json!({"store":false,"thinkingLevel":"low","responseFormat":[{"type":"image","mimeType":"image/png","aspectRatio":"1:1"}]}),
    );
    let body = test
        .provider
        .interactions("gemini-2.5-flash")
        .prepare_request(&call)
        .unwrap();
    insta::assert_json_snapshot!("interactions_request", body);
}

#[tokio::test]
async fn unary_outputs_usage_files_sources_and_custom_metadata() {
    let test = TestProvider::start_with(|mut settings| {
        settings.name = Some("custom".to_owned());
        settings
    })
    .await;
    test.mount(
        Method::POST,
        "/v1beta/interactions",
        "interactions",
        "basic",
    );
    let result = test
        .provider
        .interactions("gemini-2.5-flash")
        .do_generate(options())
        .await
        .unwrap();
    assert_eq!(
        (
            result.usage.input.total,
            result.usage.input.no_cache,
            result.usage.output.total
        ),
        (Some(10), Some(8), Some(7))
    );
    assert_eq!(
        result.provider_metadata.as_ref().unwrap()["google"],
        result.provider_metadata.as_ref().unwrap()["custom"]
    );
    assert!(matches!(
        &result.content[..],
        [
            Content::Reasoning { .. },
            Content::Text { .. },
            Content::Source(_),
            Content::File { .. }
        ]
    ));
    insta::assert_json_snapshot!("interactions_unary", result.content);
}

#[tokio::test]
async fn function_and_builtin_tools_preserve_aliases_and_signatures() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/interactions",
        "interactions",
        "tools",
    );
    let mut call = options();
    call.tools.push(ToolDefinition::provider(
        "google.code_execution",
        "python",
        Default::default(),
    ));
    let result = test
        .provider
        .interactions("gemini-2.5-flash")
        .do_generate(call)
        .await
        .unwrap();
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);
    assert!(
        matches!(&result.content[0], Content::ToolCall(call) if call.provider_executed && call.tool_name.as_str() == "python")
    );
    assert!(
        matches!(&result.content[2], Content::ToolCall(call) if !call.provider_executed && call.provider_metadata.as_ref().unwrap()["google"]["signature"] == "function-signature")
    );
}

#[tokio::test]
async fn linked_history_compacts_assistant_and_keeps_new_function_results() {
    let test = TestProvider::start().await;
    let mut part = TextPart::new("Old answer");
    part.provider_options = Some(google_options(json!({"interactionId":"interaction-1"})));
    let mut call = CallOptions::new(vec![
        PromptMessage::assistant(vec![AssistantPromptPart::Text(part)]),
        PromptMessage::tool(vec![ToolPromptPart::ToolResult(ToolResultPart {
            tool_call_id: "call-1".into(),
            tool_name: "weather".into(),
            output: ToolResultOutput::json(json!({"temperature":20})),
            provider_options: None,
        })]),
    ]);
    call.provider_options = google_options(json!({"previousInteractionId":"interaction-1"}));
    let body = test
        .provider
        .interactions("gemini-2.5-flash")
        .prepare_request(&call)
        .unwrap();
    assert_eq!(
        body["input"],
        json!([{"type":"user_input","content":[{"type":"function_result","call_id":"call-1","name":"weather","result":"{\"temperature\":20}"}]}])
    );
    call.provider_options =
        google_options(json!({"previousInteractionId":"interaction-1","store":false}));
    let body = test
        .provider
        .interactions("gemini-2.5-flash")
        .prepare_request(&call)
        .unwrap();
    assert_eq!(body["input"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn streamed_text_and_tools_obey_contract() {
    for case in ["basic", "tools"] {
        let test = TestProvider::start().await;
        test.mount(
            Method::POST,
            "/v1beta/interactions",
            "interactions",
            &format!("{case}-stream"),
        );
        let parts = collect_checked(
            test.provider
                .interactions("gemini-2.5-flash")
                .do_stream(options())
                .await
                .unwrap(),
        )
        .await;
        assert!(
            matches!(parts.last(), Some(StreamPart::Finish { .. })),
            "{parts:?}"
        );
        insta::assert_json_snapshot!(format!("interactions_stream_{case}"), parts);
    }
}

#[tokio::test]
async fn premature_eof_never_emits_executable_partial_arguments() {
    let test = TestProvider::start().await;
    test.mount_fixture(Method::POST, "/v1beta/interactions", Fixture::sse_json(&[
        json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call-1","name":"weather"}}),
        json!({"event_type":"step.delta","index":0,"delta":{"type":"arguments_delta","arguments":"{"}}),
    ]));
    let parts = collect_checked(
        test.provider
            .interactions("gemini-2.5-flash")
            .do_stream(options())
            .await
            .unwrap(),
    )
    .await;
    assert!(matches!(parts.last(), Some(StreamPart::Error { .. })));
    assert!(
        !parts
            .iter()
            .any(|part| matches!(part, StreamPart::ToolCall(_) | StreamPart::Finish { .. }))
    );
}

#[tokio::test]
async fn background_request_uses_incremental_stream() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&json!({"id":"job-1","status":"in_progress"})),
    );
    test.mount(
        Method::GET,
        "/v1beta/interactions/job-1",
        "interactions",
        "basic-stream",
    );
    let mut call = options();
    call.provider_options = google_options(
        json!({"agent":"deep-research","background":true,"agentConfig":{"type":"deep-research","thinkingSummaries":"auto"}}),
    );
    let parts = collect_checked(
        test.provider
            .interactions("deep-research")
            .do_stream(call)
            .await
            .unwrap(),
    )
    .await;
    assert!(matches!(parts.last(), Some(StreamPart::Finish { .. })));
    assert_eq!(test.server.received().len(), 2);
}

#[tokio::test]
async fn cancellation_prevents_requests() {
    let test = TestProvider::start().await;
    let call = options();
    call.cancellation.cancel();
    assert!(
        test.provider
            .interactions("gemini-2.5-flash")
            .do_generate(call)
            .await
            .is_err()
    );
    assert!(test.server.received().is_empty());
}
