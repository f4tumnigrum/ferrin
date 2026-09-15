use std::sync::Arc;

use ferrin_core::LanguageModelMiddleware;
use ferrin_core::middleware::CallKind;
use ferrin_core::middleware::MiddlewareContext;
use ferrin_core::wrap_language_model;
use ferrin_policy::FailureMode;
use ferrin_policy::capability_middleware;
use ferrin_policy::parse_allowlist;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::RecordingClient;
use super::common::engine_error;

fn model() -> Arc<MockLanguageModel> {
    MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
        .generate_repeat(GenerateResult::new(
            vec![Content::text("ok")],
            FinishReason::stop(),
        ))
        .build_shared()
}

fn options(names: &[&str]) -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text("hi")]);
    options.tools = names
        .iter()
        .map(|name| ToolDefinition::function(*name, None, json!({ "type": "object" })))
        .collect();
    options
}

fn names(options: &CallOptions) -> Vec<&str> {
    options
        .tools
        .iter()
        .map(|tool| tool.name().as_str())
        .collect()
}

#[test]
fn parses_allowlists() {
    assert_eq!(
        parse_allowlist(&json!(["a", "b"])),
        Some(["a".to_owned(), "b".to_owned()].into_iter().collect())
    );
    assert_eq!(
        parse_allowlist(&json!({ "tools": ["a"] })),
        Some(["a".to_owned()].into_iter().collect())
    );
    assert_eq!(parse_allowlist(&json!([])), Some(Default::default()));
    assert_eq!(parse_allowlist(&json!(["a", 1])), None);
    assert_eq!(parse_allowlist(&json!({ "allow": ["a"] })), None);
    assert_eq!(parse_allowlist(&json!(null)), None);
    assert_eq!(parse_allowlist(&json!(true)), None);
}

#[tokio::test]
async fn filters_tools_and_sends_the_default_input() {
    let client = RecordingClient::returning(json!(["read_file"]));
    let model = model();
    let middleware = capability_middleware(Arc::clone(&client), "ferrin/tools/allowed");
    let ctx = MiddlewareContext {
        model: model.as_ref(),
        kind: CallKind::Generate,
    };
    let mut input = options(&["read_file", "delete_file"]);
    input.tool_choice = Some(ToolChoice::Tool {
        tool_name: "delete_file".into(),
    });
    let output = middleware.transform_params(input, ctx).await.unwrap();
    assert_eq!(names(&output), vec!["read_file"]);
    assert_eq!(output.tool_choice, None);

    let calls = client.calls();
    assert_eq!(calls[0].0, "ferrin/tools/allowed");
    assert_eq!(
        calls[0].1,
        json!({
            "model": { "provider": "mock", "model_id": "mock-model" },
            "call": "generate",
            "tools": [
                { "name": "read_file", "provider_defined": false },
                { "name": "delete_file", "provider_defined": false },
            ],
            "tool_choice": { "type": "tool", "tool_name": "delete_file" },
        })
    );
}

#[tokio::test]
async fn keeps_forced_tools_that_remain_allowed() {
    let middleware = capability_middleware(
        RecordingClient::returning(json!({ "tools": ["read_file"] })),
        "p",
    );
    let model = model();
    let ctx = MiddlewareContext {
        model: model.as_ref(),
        kind: CallKind::Stream,
    };
    let mut input = options(&["read_file", "delete_file"]);
    input.tool_choice = Some(ToolChoice::Tool {
        tool_name: "read_file".into(),
    });
    let output = middleware.transform_params(input, ctx).await.unwrap();
    assert_eq!(names(&output), vec!["read_file"]);
    assert_eq!(
        output.tool_choice,
        Some(ToolChoice::Tool {
            tool_name: "read_file".into(),
        })
    );
}

#[tokio::test]
async fn failures_clear_tools_unless_falling_through() {
    let model = model();
    let ctx = MiddlewareContext {
        model: model.as_ref(),
        kind: CallKind::Generate,
    };
    for client in [
        RecordingClient::failing(engine_error()),
        RecordingClient::returning(json!({ "decision": "allow" })),
    ] {
        let middleware = capability_middleware(Arc::clone(&client), "p");
        let mut input = options(&["read_file"]);
        input.tool_choice = Some(ToolChoice::Required);
        let output = middleware.transform_params(input, ctx).await.unwrap();
        assert!(output.tools.is_empty());
        assert_eq!(output.tool_choice, None);

        let middleware =
            capability_middleware(Arc::clone(&client), "p").on_error(FailureMode::FallThrough);
        let mut input = options(&["read_file"]);
        input.tool_choice = Some(ToolChoice::Required);
        let output = middleware.transform_params(input, ctx).await.unwrap();
        assert_eq!(names(&output), vec!["read_file"]);
        assert_eq!(output.tool_choice, Some(ToolChoice::Required));
    }
}

#[tokio::test]
async fn calls_without_tools_are_not_evaluated() {
    let client = RecordingClient::returning(json!([]));
    let model = model();
    let wrapped = wrap_language_model(
        Arc::clone(&model) as Arc<dyn DynLanguageModel>,
        [Arc::new(capability_middleware(Arc::clone(&client), "p"))
            as Arc<dyn LanguageModelMiddleware>],
    );
    wrapped.do_generate(options(&[])).await.unwrap();
    assert!(client.calls().is_empty());

    wrapped.do_generate(options(&["x"])).await.unwrap();
    assert_eq!(client.calls().len(), 1);
    assert!(model.generate_calls()[1].tools.is_empty());
}
