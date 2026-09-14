//! Replay of approval responses found in the message history.

use super::ApprovalContext;
use super::ApprovalStatus;
use super::ParsedToolCall;
use super::StepContent;
use super::ToolErrorInfo;
use super::ToolExecutionError;
use super::ToolOutputDenied;
use super::approval::collect::collect_tool_approvals;
use super::approval::resolve_approval;
use super::approval::signature;
use super::response_messages::tool_error_output;
use super::run::LoopContext;
use super::tools::execute_tools;
use crate::cancel::CallCancellation;
use crate::error::Error;
use ferrin_message::Message;
use ferrin_message::ToolPart;
use ferrin_message::ToolResultOutput;
use ferrin_message::ToolResultPart;
use ferrin_tool::ErrorMode;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolSet;
use ferrin_tool::model_output::create_tool_model_output;
use std::sync::Arc;

/// Replays approval responses found in the message history: verifies
/// signatures, re-validates inputs, re-resolves the policy, executes approved
/// calls and records denied ones.
pub(crate) async fn replay_approvals(
    ctx: &LoopContext,
    messages: &[Message],
    cancellation: &CallCancellation,
) -> Result<Vec<StepContent>, Error> {
    let collected = collect_tool_approvals(messages)?;
    if collected.approved.is_empty() && collected.denied.is_empty() {
        return Ok(Vec::new());
    }
    let messages_arc: Arc<[Message]> = Arc::from(messages.to_vec());
    let tools_context = ctx.config.tools_context.clone();
    let approval_ctx = ApprovalContext {
        messages,
        tools_context: tools_context.as_ref(),
    };
    let mut outputs: Vec<StepContent> = Vec::new();
    let mut denied = collected.denied;
    let mut to_execute: Vec<ParsedToolCall> = Vec::new();

    for mut approval in collected.approved {
        if approval.tool_call.provider_executed {
            continue;
        }
        let tool = ctx
            .execution_tools
            .get(approval.tool_call.tool_name.as_str());
        if let Some(secret) = &ctx.config.tool_approval_secret {
            let fields = signature::SignedFields {
                approval_id: &approval.request.approval_id,
                tool_call_id: &approval.tool_call.tool_call_id,
                tool_name: &approval.tool_call.tool_name,
                input: &approval.tool_call.input,
            };
            let valid = approval
                .request
                .signature
                .as_deref()
                .is_some_and(|signature| signature::verify(secret, fields, signature));
            if !valid {
                let message = if approval.request.signature.is_none() {
                    "missing signature"
                } else {
                    "invalid signature"
                };
                return Err(Error::InvalidToolApproval {
                    approval_id: approval.request.approval_id.clone(),
                    message: message.to_owned(),
                });
            }
        }
        let mut call = ParsedToolCall::new(
            approval.tool_call.tool_call_id.clone(),
            approval.tool_call.tool_name.clone(),
            approval.tool_call.input.clone(),
        );
        call.dynamic = tool.is_some_and(|tool| tool.kind().is_dynamic());
        if let Some(tool) = tool
            && tool.is_executable()
        {
            match tool.validate_input(&call.tool_name, call.input.clone()) {
                Ok(valid) => call.input = valid,
                Err(cause) => {
                    let error = Error::invalid_tool_input(
                        call.tool_name.clone(),
                        call.input.to_string(),
                        Box::new(cause),
                    );
                    outputs.push(StepContent::ToolError(ToolExecutionError {
                        tool_call_id: call.tool_call_id,
                        tool_name: call.tool_name,
                        input: call.input,
                        error: ToolErrorInfo::text(error.to_string()),
                        provider_executed: false,
                        dynamic: call.dynamic,
                        provider_metadata: None,
                    }));
                    continue;
                }
            }
        }
        let status = resolve_approval(
            &call,
            tool.map(AsRef::as_ref),
            ctx.config.tool_approval.as_deref(),
            approval_ctx,
            || {
                tool.map_or_else(
                    || ToolContext::new(call.tool_call_id.clone()),
                    |tool| {
                        ctx.tool_context(
                            tool,
                            &call.tool_call_id,
                            &call.tool_name,
                            &messages_arc,
                            tools_context.as_ref(),
                            cancellation,
                        )
                    },
                )
            },
        )
        .await;
        if let ApprovalStatus::Denied { reason } = status {
            approval.response.approved = false;
            if reason.is_some() {
                approval.response.reason = reason;
            }
            denied.push(approval);
            continue;
        }
        to_execute.push(call);
    }

    outputs
        .extend(execute_tools(ctx, to_execute, messages_arc, tools_context, cancellation).await?);
    for approval in denied
        .into_iter()
        .filter(|approval| approval.existing_result.is_none())
    {
        let dynamic = ctx
            .execution_tools
            .get(approval.tool_call.tool_name.as_str())
            .is_some_and(|tool| tool.kind().is_dynamic());
        outputs.push(StepContent::ToolOutputDenied(ToolOutputDenied {
            tool_call_id: approval.tool_call.tool_call_id,
            tool_name: approval.tool_call.tool_name,
            input: approval.tool_call.input,
            reason: approval.response.reason,
            provider_executed: approval.tool_call.provider_executed,
            dynamic,
        }));
    }
    Ok(outputs)
}

/// The tool message carrying replayed approval outputs.
pub(crate) fn replay_tool_message(outputs: &[StepContent], tools: &ToolSet) -> Option<Message> {
    let parts: Vec<ToolPart> = outputs
        .iter()
        .filter_map(|part| {
            let result = match part {
                StepContent::ToolResult(result) => ToolResultPart {
                    tool_call_id: result.tool_call_id.clone(),
                    tool_name: result.tool_name.clone(),
                    output: create_tool_model_output(
                        tools.get(result.tool_name.as_str()).map(AsRef::as_ref),
                        &result.tool_call_id,
                        &result.input,
                        &result.output,
                        ErrorMode::None,
                    ),
                    provider_options: None,
                },
                StepContent::ToolError(error) => ToolResultPart {
                    tool_call_id: error.tool_call_id.clone(),
                    tool_name: error.tool_name.clone(),
                    output: tool_error_output(&error.error),
                    provider_options: None,
                },
                StepContent::ToolOutputDenied(denied) => ToolResultPart {
                    tool_call_id: denied.tool_call_id.clone(),
                    tool_name: denied.tool_name.clone(),
                    output: ToolResultOutput::execution_denied(denied.reason.clone()),
                    provider_options: None,
                },
                _ => return None,
            };
            Some(ToolPart::ToolResult(result))
        })
        .collect();
    (!parts.is_empty()).then(|| Message::tool(parts))
}
