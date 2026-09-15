use std::sync::Arc;

use ferrin_core::StepContent;
use ferrin_core::generate_text;
use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::step_count;
use ferrin_message::Message;
use ferrin_policy::FailureMode;
use ferrin_policy::default_input;
use ferrin_policy::policy_approval;
use ferrin_policy::policy_client;
use ferrin_policy::with_default;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCall;
use ferrin_testing::MockLanguageModel;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::RecordingClient;
use super::common::delete_call;
use super::common::empty_context;
use super::common::engine_error;

#[tokio::test]
async fn sends_the_default_input_and_maps_the_decision() {
    let client = RecordingClient::returning(json!({ "decision": "deny", "reason": "nope" }));
    let policy = policy_approval(Arc::clone(&client), "ferrin/tools/decision");
    let messages = vec![Message::user("delete it")];
    let tools_context = json!({ "tenant": "acme" });
    let ctx = ApprovalContext {
        messages: &messages,
        tools_context: Some(&tools_context),
    };
    let status = policy.resolve(&delete_call(), ctx).await;
    assert_eq!(status, Some(ApprovalStatus::denied().with_reason("nope")));

    let calls = client.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ferrin/tools/decision");
    assert_eq!(
        calls[0].1,
        json!({
            "tool": {
                "name": "delete_file",
                "tool_call_id": "call-1",
                "dynamic": false,
                "provider_executed": false,
                "invalid": false,
            },
            "input": { "path": "/tmp/x" },
            "messages": serde_json::to_value(&messages).unwrap(),
            "tools_context": { "tenant": "acme" },
        })
    );
    assert_eq!(calls[0].1, default_input(&delete_call(), &ctx));
}

#[tokio::test]
async fn not_applicable_falls_through() {
    let client = RecordingClient::returning(JsonValue::Null);
    let policy = policy_approval(client, "ferrin/tools/decision");
    assert_eq!(policy.resolve(&delete_call(), empty_context()).await, None);
}

#[tokio::test]
async fn evaluation_errors_deny_by_default_and_can_fall_through() {
    let policy = policy_approval(RecordingClient::failing(engine_error()), "p");
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::denied().with_reason("policy evaluation failed"))
    );

    let policy = policy_approval(RecordingClient::failing(engine_error()), "p")
        .on_error(FailureMode::FallThrough);
    assert_eq!(policy.resolve(&delete_call(), empty_context()).await, None);
}

#[tokio::test]
async fn custom_input_replaces_the_default() {
    let client = RecordingClient::returning(json!(true));
    let policy = policy_approval(Arc::clone(&client), "p")
        .to_input(|call, _ctx| json!({ "name": call.tool_name }));
    let status = policy.resolve(&delete_call(), empty_context()).await;
    assert_eq!(status, Some(ApprovalStatus::approved()));
    assert_eq!(client.calls()[0].1, json!({ "name": "delete_file" }));
}

#[tokio::test]
async fn with_default_fills_undecided_calls() {
    let inner = policy_approval(RecordingClient::returning(JsonValue::Null), "p");
    let policy = with_default(inner, ApprovalStatus::user_approval());
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::user_approval())
    );

    let inner = policy_approval(RecordingClient::returning(json!(true)), "p");
    let policy = with_default(inner, ApprovalStatus::user_approval());
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::approved())
    );
}

fn tools() -> ToolSet {
    let tool = Tool::function_with_schema(Schema::from_json_schema(json!({
        "type": "object",
        "properties": { "path": { "type": "string" } },
        "required": ["path"]
    })))
    .execute(|input: JsonValue, _ctx: ToolContext| async move {
        Ok::<_, ToolError>(json!({ "deleted": input["path"] }))
    })
    .build();
    ToolSet::new().insert("delete_file", tool).unwrap()
}

fn model() -> Arc<MockLanguageModel> {
    let call = GenerateResult::new(
        vec![Content::ToolCall(ToolCall::new(
            "call-1",
            "delete_file",
            json!({ "path": "/tmp/x" }).to_string(),
        ))],
        FinishReason::tool_calls(),
    );
    let done = GenerateResult::new(vec![Content::text("done")], FinishReason::stop());
    MockLanguageModel::builder()
        .generate(call)
        .generate(done)
        .build_shared()
}

fn kinds(content: &[StepContent]) -> Vec<&'static str> {
    content.iter().map(StepContent::kind_name).collect()
}

#[tokio::test]
async fn denied_calls_do_not_execute_in_the_generation_loop() {
    let client = policy_client(|_path, input| {
        let path = input["input"]["path"].as_str().unwrap_or_default();
        Ok(json!({
            "decision": if path.starts_with("/tmp/") { "deny" } else { "allow" },
            "reason": "temporary files are protected",
        }))
    });
    let result = generate_text(model())
        .prompt("delete it")
        .tools(tools())
        .tool_approval(policy_approval(client, "ferrin/tools/decision"))
        .stop_when(step_count(3))
        .await
        .unwrap();
    assert_eq!(result.text(), "done");
    assert_eq!(result.steps.len(), 2);
    // The denial is recorded as an approval response; no `tool-result`
    // appears because the tool never ran.
    assert_eq!(
        kinds(&result.steps[0].content),
        vec![
            "tool-call",
            "tool-approval-request",
            "tool-approval-response",
        ]
    );
    let response = match &result.steps[0].content[2] {
        StepContent::ToolApprovalResponse(response) => response,
        other => panic!("unexpected content {other:?}"),
    };
    assert!(!response.approved);
    assert_eq!(
        response.reason.as_deref(),
        Some("temporary files are protected")
    );
}

#[tokio::test]
async fn allowed_calls_execute_and_approval_requests_stop_the_loop() {
    let allow = policy_client(|_path, _input| Ok(json!({ "decision": "allow" })));
    let result = generate_text(model())
        .prompt("delete it")
        .tools(tools())
        .tool_approval(policy_approval(allow, "p"))
        .stop_when(step_count(3))
        .await
        .unwrap();
    assert_eq!(
        kinds(&result.steps[0].content),
        vec![
            "tool-call",
            "tool-approval-request",
            "tool-approval-response",
            "tool-result",
        ]
    );

    let ask = policy_client(|_path, _input| Ok(json!({ "decision": "requires-approval" })));
    let result = generate_text(model())
        .prompt("delete it")
        .tools(tools())
        .tool_approval(policy_approval(ask, "p"))
        .stop_when(step_count(3))
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(
        kinds(&result.steps[0].content),
        vec!["tool-call", "tool-approval-request"]
    );
}
