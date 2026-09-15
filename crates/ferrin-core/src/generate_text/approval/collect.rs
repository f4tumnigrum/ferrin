//! Collection of approval responses from the trailing tool message.

use std::collections::HashMap;

use ferrin_message::AssistantPart;
use ferrin_message::Message;
use ferrin_message::ToolApprovalRequest;
use ferrin_message::ToolApprovalResponse;
use ferrin_message::ToolCallPart;
use ferrin_message::ToolPart;
use ferrin_message::ToolResultOutput;
use ferrin_message::ToolResultPart;

use crate::error::Error;

/// One approval response matched with its request and tool call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolApproval {
    pub(crate) tool_call: ToolCallPart,
    pub(crate) request: ToolApprovalRequest,
    pub(crate) response: ToolApprovalResponse,
    pub(crate) existing_result: Option<ToolResultPart>,
}

/// Approvals split by decision.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CollectedApprovals {
    pub(crate) approved: Vec<ToolApproval>,
    pub(crate) denied: Vec<ToolApproval>,
}

/// Collects the approval responses of the last message when it is a tool
/// message.
///
/// # Errors
///
/// [`Error::InvalidToolApproval`] when a response has no matching request,
/// [`Error::ToolCallNotFoundForApproval`] when the request's tool call is
/// missing.
pub(crate) fn collect_tool_approvals(messages: &[Message]) -> Result<CollectedApprovals, Error> {
    let Some(Message::Tool(last)) = messages.last() else {
        return Ok(CollectedApprovals::default());
    };
    let responses: Vec<&ToolApprovalResponse> = last
        .content
        .iter()
        .filter_map(ToolPart::as_tool_approval_response)
        .collect();
    if responses.is_empty() {
        return Ok(CollectedApprovals::default());
    }

    let mut tool_calls: HashMap<&str, &ToolCallPart> = HashMap::new();
    let mut requests: HashMap<&str, &ToolApprovalRequest> = HashMap::new();
    let mut results: HashMap<&str, &ToolResultPart> = HashMap::new();
    for message in messages {
        match message {
            Message::Assistant(assistant) => {
                for part in assistant.content.as_parts().unwrap_or_default() {
                    match part {
                        AssistantPart::ToolCall(call) => {
                            tool_calls.insert(call.tool_call_id.as_str(), call);
                        }
                        AssistantPart::ToolApprovalRequest(request) => {
                            requests.insert(request.approval_id.as_str(), request);
                        }
                        _ => {}
                    }
                }
            }
            Message::Tool(tool) => {
                for part in &tool.content {
                    if let ToolPart::ToolResult(result) = part {
                        results.insert(result.tool_call_id.as_str(), result);
                    }
                }
            }
            _ => {}
        }
    }

    let mut collected = CollectedApprovals::default();
    let mut approval_decisions = HashMap::new();
    let mut call_decisions = HashMap::new();
    for response in responses {
        let Some(request) = requests.get(response.approval_id.as_str()) else {
            return Err(Error::InvalidToolApproval {
                approval_id: response.approval_id.clone(),
                message: "no approval request found for the approval response".to_owned(),
            });
        };
        let Some(tool_call) = tool_calls.get(request.tool_call_id.as_str()) else {
            return Err(Error::ToolCallNotFoundForApproval {
                tool_call_id: request.tool_call_id.clone(),
                approval_id: response.approval_id.clone(),
            });
        };
        let decision = (response.approved, response.provider_executed);
        let duplicate_approval = approval_decisions.insert(response.approval_id.as_str(), decision);
        let duplicate_call = call_decisions.insert(tool_call.tool_call_id.as_str(), decision);
        if [duplicate_approval, duplicate_call]
            .into_iter()
            .flatten()
            .any(|previous| previous != decision)
        {
            return Err(Error::InvalidToolApproval {
                approval_id: response.approval_id.clone(),
                message: "conflicting approval decisions for the same tool call".to_owned(),
            });
        }
        if duplicate_approval.is_some() || duplicate_call.is_some() {
            continue;
        }
        let existing = results.get(tool_call.tool_call_id.as_str()).copied();
        if let Some(existing) = existing
            && (response.approved
                || !matches!(existing.output, ToolResultOutput::ExecutionDenied { .. }))
        {
            continue;
        }
        let approval = ToolApproval {
            tool_call: (*tool_call).clone(),
            request: (*request).clone(),
            response: response.clone(),
            existing_result: existing.cloned(),
        };
        if response.approved {
            collected.approved.push(approval);
        } else {
            collected.denied.push(approval);
        }
    }
    Ok(collected)
}
