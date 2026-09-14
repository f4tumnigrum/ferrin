//! Assembly of the messages a step sends back to the model.

use std::collections::HashMap;

use ferrin_message::AssistantPart;
use ferrin_message::FilePart;
use ferrin_message::FileSource;
use ferrin_message::Message;
use ferrin_message::ReasoningFilePart;
use ferrin_message::ToolApprovalRequest;
use ferrin_message::ToolApprovalResponse;
use ferrin_message::ToolPart;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::language_model::prompt::CustomPart;
use ferrin_spec::language_model::prompt::ReasoningPart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_tool::ErrorMode;
use ferrin_tool::ToolSet;
use ferrin_tool::model_output::create_tool_model_output;

use super::StepContent;
use super::ToolErrorInfo;

/// Converts step content into the assistant message (and the tool message
/// for client tool outputs) to append to the history.
pub(crate) fn to_response_messages(content: &[StepContent], tools: &ToolSet) -> Vec<Message> {
    let mut messages = Vec::with_capacity(2);
    let mut tool_call_order: HashMap<&ToolCallId, usize> = HashMap::new();
    let mut assistant: Vec<AssistantPart> = Vec::new();

    for part in content {
        match part {
            StepContent::Text { text, .. } if text.is_empty() => {}
            StepContent::Text {
                text,
                provider_metadata,
            } => assistant.push(AssistantPart::Text(TextPart {
                text: text.clone(),
                provider_options: provider_metadata.clone(),
            })),
            StepContent::Reasoning {
                text,
                provider_metadata,
            } => assistant.push(AssistantPart::Reasoning(ReasoningPart {
                text: text.clone(),
                provider_options: provider_metadata.clone(),
            })),
            StepContent::ReasoningFile(file) => {
                assistant.push(AssistantPart::ReasoningFile(ReasoningFilePart {
                    data: FileSource::from(file.data.clone()),
                    media_type: file.media_type.clone(),
                    provider_options: file.provider_metadata.clone(),
                }));
            }
            StepContent::File(file) => assistant.push(AssistantPart::File(FilePart {
                data: FileSource::from(file.data.clone()),
                media_type: file.media_type.clone(),
                filename: file.filename.clone(),
                provider_options: file.provider_metadata.clone(),
            })),
            StepContent::Custom {
                kind,
                provider_metadata,
            } => assistant.push(AssistantPart::Custom(CustomPart {
                kind: kind.clone(),
                provider_options: provider_metadata.clone(),
            })),
            StepContent::Source(_) => {}
            StepContent::ToolCall(call) => {
                let next = tool_call_order.len();
                tool_call_order.entry(&call.tool_call_id).or_insert(next);
                let input = if call.invalid && !call.input.is_object() {
                    JsonValue::Object(serde_json::Map::new())
                } else {
                    call.input.clone()
                };
                assistant.push(AssistantPart::ToolCall(ToolCallPart {
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    input,
                    provider_executed: call.provider_executed,
                    provider_options: call.provider_metadata.clone(),
                }));
            }
            StepContent::ToolResult(result) if result.provider_executed => {
                let output = create_tool_model_output(
                    tools.get(result.tool_name.as_str()).map(AsRef::as_ref),
                    &result.tool_call_id,
                    &result.input,
                    &result.output,
                    ErrorMode::None,
                );
                assistant.push(AssistantPart::ToolResult(ToolResultPart {
                    tool_call_id: result.tool_call_id.clone(),
                    tool_name: result.tool_name.clone(),
                    output,
                    provider_options: result.provider_metadata.clone(),
                }));
            }
            StepContent::ToolError(error) if error.provider_executed => {
                let output = create_tool_model_output(
                    tools.get(error.tool_name.as_str()).map(AsRef::as_ref),
                    &error.tool_call_id,
                    &error.input,
                    &error.error.to_json_value(),
                    ErrorMode::Json,
                );
                assistant.push(AssistantPart::ToolResult(ToolResultPart {
                    tool_call_id: error.tool_call_id.clone(),
                    tool_name: error.tool_name.clone(),
                    output,
                    provider_options: error.provider_metadata.clone(),
                }));
            }
            StepContent::ToolApprovalRequest(request) => {
                let mut part = ToolApprovalRequest::new(
                    request.approval_id.clone(),
                    request.tool_call.tool_call_id.clone(),
                );
                part.reason.clone_from(&request.reason);
                part.is_automatic = request.is_automatic;
                part.signature.clone_from(&request.signature);
                assistant.push(AssistantPart::ToolApprovalRequest(part));
            }
            StepContent::ToolResult(_)
            | StepContent::ToolError(_)
            | StepContent::ToolApprovalResponse(_)
            | StepContent::ToolOutputDenied(_) => {}
            #[allow(unreachable_patterns, reason = "the content enum is non-exhaustive")]
            _ => {}
        }
    }
    if !assistant.is_empty() {
        messages.push(Message::assistant_parts(assistant));
    }

    let mut tool_parts: Vec<(usize, ToolPart)> = Vec::new();
    let order_of = |id: &ToolCallId| tool_call_order.get(id).copied().unwrap_or(usize::MAX);
    for part in content {
        match part {
            StepContent::ToolApprovalResponse(response) => {
                let position = order_of(&response.tool_call.tool_call_id);
                let mut approval = if response.approved {
                    ToolApprovalResponse::approved(response.approval_id.clone())
                } else {
                    ToolApprovalResponse::denied(response.approval_id.clone())
                };
                approval.reason.clone_from(&response.reason);
                approval.provider_executed = response.provider_executed;
                tool_parts.push((position, ToolPart::ToolApprovalResponse(approval)));
                if !response.approved {
                    tool_parts.push((
                        position,
                        ToolPart::ToolResult(ToolResultPart {
                            tool_call_id: response.tool_call.tool_call_id.clone(),
                            tool_name: response.tool_call.tool_name.clone(),
                            output: ToolResultOutput::execution_denied(response.reason.clone()),
                            provider_options: None,
                        }),
                    ));
                }
            }
            StepContent::ToolResult(result) if !result.provider_executed => {
                let output = create_tool_model_output(
                    tools.get(result.tool_name.as_str()).map(AsRef::as_ref),
                    &result.tool_call_id,
                    &result.input,
                    &result.output,
                    ErrorMode::None,
                );
                tool_parts.push((
                    order_of(&result.tool_call_id),
                    ToolPart::ToolResult(ToolResultPart {
                        tool_call_id: result.tool_call_id.clone(),
                        tool_name: result.tool_name.clone(),
                        output,
                        provider_options: result.provider_metadata.clone(),
                    }),
                ));
            }
            StepContent::ToolError(error) if !error.provider_executed => {
                tool_parts.push((
                    order_of(&error.tool_call_id),
                    ToolPart::ToolResult(ToolResultPart {
                        tool_call_id: error.tool_call_id.clone(),
                        tool_name: error.tool_name.clone(),
                        output: tool_error_output(&error.error),
                        provider_options: error.provider_metadata.clone(),
                    }),
                ));
            }
            StepContent::ToolOutputDenied(denied) if !denied.provider_executed => {
                tool_parts.push((
                    order_of(&denied.tool_call_id),
                    ToolPart::ToolResult(ToolResultPart {
                        tool_call_id: denied.tool_call_id.clone(),
                        tool_name: denied.tool_name.clone(),
                        output: ToolResultOutput::execution_denied(denied.reason.clone()),
                        provider_options: None,
                    }),
                ));
            }
            _ => {}
        }
    }
    if !tool_parts.is_empty() {
        tool_parts.sort_by_key(|(position, _)| *position);
        messages.push(Message::tool(tool_parts.into_iter().map(|(_, part)| part)));
    }
    messages
}

/// Renders a tool error as the model-facing output (`error-text` for
/// textual errors, `error-json` for structured payloads).
pub(crate) fn tool_error_output(error: &ToolErrorInfo) -> ToolResultOutput {
    match error {
        ToolErrorInfo::Text { message } => ToolResultOutput::error_text(message.clone()),
        ToolErrorInfo::Json { value } => ToolResultOutput::error_json(value.clone()),
        #[allow(unreachable_patterns, reason = "the error enum is non-exhaustive")]
        _ => ToolResultOutput::error_text(error.message()),
    }
}
