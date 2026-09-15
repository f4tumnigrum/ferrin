use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::StepContent;
use ferrin_core::generate_text;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::approval_policy;
use ferrin_core::step_count;
use ferrin_message::AssistantContent;
use ferrin_message::AssistantPart;
use ferrin_message::Message;
use ferrin_message::ToolApprovalResponse;
use ferrin_spec::JsonValue;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_tool::NeedsApproval;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use secrecy::SecretBox;
use serde_json::json;

use super::common::kinds;
use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;

fn guarded_tools() -> ToolSet {
    let tool = Tool::function_with_schema(Schema::from_json_schema(json!({
        "type": "object",
        "properties": { "path": { "type": "string" } },
        "required": ["path"]
    })))
    .needs_approval(NeedsApproval::Always)
    .execute(|input: JsonValue, _ctx: ToolContext| async move {
        Ok::<_, ToolError>(json!({ "deleted": input["path"] }))
    })
    .build();
    ToolSet::new().insert("delete_file", tool).unwrap()
}

fn secret() -> SecretBox<[u8]> {
    SecretBox::new(Box::from(b"approval-secret".as_slice()))
}

/// Runs the first call and returns the history including the approval
/// request, plus the approval id.
async fn request_approval(with_secret: bool) -> (Vec<Message>, String) {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "delete_file",
            &json!({ "path": "/tmp/x" }),
        ))
        .build_shared();
    let mut call = generate_text(Arc::clone(&model))
        .prompt("delete it")
        .tools(guarded_tools())
        .stop_when(step_count(5));
    if with_secret {
        call = call.tool_approval_secret(secret());
    }
    let result = call.await.unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(
        kinds(&result.steps[0].content),
        vec!["tool-call", "tool-approval-request"]
    );
    let approval_id = match &result.steps[0].content[1] {
        StepContent::ToolApprovalRequest(request) => {
            assert_eq!(request.tool_call.tool_call_id.as_str(), "call-1");
            assert_eq!(request.signature.is_some(), with_secret);
            request.approval_id.to_string()
        }
        other => panic!("unexpected content {other:?}"),
    };
    let mut history = vec![Message::user("delete it")];
    history.extend(result.response_messages());
    assert_eq!(history.len(), 2);
    (history, approval_id)
}

#[tokio::test]
async fn approved_replay_executes_the_tool_before_calling_the_model() {
    let (mut history, approval_id) = request_approval(false).await;
    history.push(Message::tool([ToolApprovalResponse::approved(approval_id)]));

    let model = mock().generate(text_result("gone")).build_shared();
    let result = generate_text(Arc::clone(&model))
        .messages(history)
        .tools(guarded_tools())
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 1);
    // Replayed outputs are not part of the step content: they are sent to
    // the model as the first (tool) message of the step.
    assert_eq!(kinds(&result.steps[0].content), vec!["text"]);
    let messages = &result.steps[0].response.messages;
    assert_eq!(messages.len(), 2);
    let tool_message = messages[0].as_tool().unwrap();
    let part = tool_message.content[0].as_tool_result().unwrap();
    assert_eq!(part.tool_call_id.as_str(), "call-1");
    assert_eq!(
        part.output,
        ToolResultOutput::json(json!({ "deleted": "/tmp/x" }))
    );

    let prompt = &model.generate_calls()[0].prompt;
    match prompt.last().unwrap() {
        PromptMessage::Tool { content, .. } => match &content[0] {
            ToolPromptPart::ToolResult(part) => {
                assert_eq!(
                    part.output,
                    ToolResultOutput::json(json!({ "deleted": "/tmp/x" }))
                );
            }
            other => panic!("unexpected part {other:?}"),
        },
        other => panic!("unexpected message {other:?}"),
    }
}

#[tokio::test]
async fn denied_replay_reports_execution_denied() {
    let (mut history, approval_id) = request_approval(false).await;
    history.push(Message::tool([
        ToolApprovalResponse::denied(approval_id).with_reason("not today")
    ]));

    let model = mock().generate(text_result("ok")).build_shared();
    let result = generate_text(Arc::clone(&model))
        .messages(history)
        .tools(guarded_tools())
        .await
        .unwrap();
    assert_eq!(kinds(&result.steps[0].content), vec!["text"]);
    let tool_message = result.steps[0].response.messages[0].as_tool().unwrap();
    let part = tool_message.content[0].as_tool_result().unwrap();
    assert_eq!(
        part.output,
        ToolResultOutput::execution_denied(Some("not today".to_owned()))
    );
    let prompt = &model.generate_calls()[0].prompt;
    match prompt.last().unwrap() {
        PromptMessage::Tool { content, .. } => match &content[0] {
            ToolPromptPart::ToolResult(part) => {
                assert_eq!(
                    part.output,
                    ToolResultOutput::execution_denied(Some("not today".to_owned()))
                );
            }
            other => panic!("unexpected part {other:?}"),
        },
        other => panic!("unexpected message {other:?}"),
    }
}

#[tokio::test]
async fn tampered_history_fails_signature_verification() {
    let (mut history, approval_id) = request_approval(true).await;
    if let Message::Assistant(assistant) = &mut history[1]
        && let AssistantContent::Parts(parts) = &mut assistant.content
    {
        for part in parts.iter_mut() {
            if let AssistantPart::ToolCall(call) = part {
                call.input = json!({ "path": "/etc/passwd" });
            }
        }
    }
    history.push(Message::tool([ToolApprovalResponse::approved(approval_id)]));

    let model = mock().generate(text_result("never")).build_shared();
    let error = generate_text(Arc::clone(&model))
        .messages(history)
        .tools(guarded_tools())
        .tool_approval_secret(secret())
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::InvalidToolApproval { .. }),
        "{error:?}"
    );
    assert_eq!(model.call_count(), 0);
}

#[tokio::test]
async fn unknown_approval_ids_are_rejected() {
    let (mut history, _) = request_approval(false).await;
    history.push(Message::tool([ToolApprovalResponse::approved("missing")]));
    let error = generate_text(mock().generate(text_result("x")).build_shared())
        .messages(history)
        .tools(guarded_tools())
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::ToolCallNotFoundForApproval { .. } | Error::InvalidToolApproval { .. }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn automatic_policy_decisions_skip_the_user() {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "delete_file",
            &json!({ "path": "/tmp/x" }),
        ))
        .generate(text_result("done"))
        .build_shared();
    let result = generate_text(Arc::clone(&model))
        .prompt("delete it")
        .tools(guarded_tools())
        .tool_approval(approval_policy(|_call, _ctx| {
            Some(ApprovalStatus::denied().with_reason("policy"))
        }))
        .stop_when(step_count(5))
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 2);
    let first = &result.steps[0];
    assert_eq!(
        kinds(&first.content),
        vec![
            "tool-call",
            "tool-approval-request",
            "tool-approval-response"
        ]
    );
    match (&first.content[1], &first.content[2]) {
        (
            StepContent::ToolApprovalRequest(request),
            StepContent::ToolApprovalResponse(response),
        ) => {
            assert!(request.is_automatic);
            assert_eq!(request.approval_id, response.approval_id);
            assert!(!response.approved);
            assert_eq!(response.reason.as_deref(), Some("policy"));
        }
        other => panic!("unexpected content {other:?}"),
    }
    let denied = first.response.messages[1].as_tool().unwrap();
    assert!(denied.content.iter().any(|part| {
        part.as_tool_result().is_some_and(|result| {
            result.output == ToolResultOutput::execution_denied(Some("policy".to_owned()))
        })
    }));
    assert_eq!(result.text(), "done");

    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "delete_file",
            &json!({ "path": "/tmp/x" }),
        ))
        .generate(text_result("done"))
        .build_shared();
    let result = generate_text(Arc::clone(&model))
        .prompt("delete it")
        .tools(guarded_tools())
        .tool_approval(ApprovalStatus::approved())
        .stop_when(step_count(5))
        .await
        .unwrap();
    assert_eq!(
        kinds(&result.steps[0].content),
        vec![
            "tool-call",
            "tool-approval-request",
            "tool-approval-response",
            "tool-result"
        ]
    );
}

#[tokio::test]
async fn duplicate_signed_approval_responses_execute_once_in_both_loops() {
    for streaming in [false, true] {
        let (mut history, approval_id) = request_approval(true).await;
        let response = ToolApprovalResponse::approved(approval_id);
        history.push(Message::tool([response.clone(), response]));
        let executions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&executions);
        let tools = ToolSet::new()
            .insert(
                "delete_file",
                Tool::function_with_schema(Schema::from_json_schema(json!({"type":"object"})))
                    .needs_approval(NeedsApproval::Always)
                    .execute(move |_: JsonValue, _: ToolContext| {
                        let count = Arc::clone(&count);
                        async move {
                            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Ok::<_, ToolError>(json!("done"))
                        }
                    })
                    .build(),
            )
            .unwrap();
        let model = mock()
            .generate(text_result("done"))
            .stream(ferrin_testing::text_parts(
                ["done"],
                ferrin_spec::Usage::default(),
            ))
            .build_shared();
        let result = if streaming {
            ferrin_core::stream_text(model)
                .messages(history)
                .tools(tools)
                .tool_approval_secret(secret())
                .await
                .unwrap()
                .consume()
                .await
                .unwrap()
        } else {
            generate_text(model)
                .messages(history)
                .tools(tools)
                .tool_approval_secret(secret())
                .await
                .unwrap()
        };
        assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            result.response_messages()[0]
                .as_tool()
                .unwrap()
                .content
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn conflicting_approval_decisions_fail_before_model_or_tool_execution() {
    for reverse in [false, true] {
        let (mut history, approval_id) = request_approval(true).await;
        let mut responses = vec![
            ToolApprovalResponse::approved(approval_id.clone()),
            ToolApprovalResponse::denied(approval_id),
        ];
        if reverse {
            responses.reverse();
        }
        history.push(Message::tool(responses));
        let model = mock().generate(text_result("unreachable")).build_shared();
        let error = generate_text(Arc::clone(&model))
            .messages(history)
            .tools(guarded_tools())
            .tool_approval_secret(secret())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::InvalidToolApproval { .. }));
        assert_eq!(model.call_count(), 0);
    }
}
