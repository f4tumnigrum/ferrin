//! Handling of `response.output_item.added` / `.done` events.
//!
//! Advanced tool behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use std::collections::BTreeMap;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::ToolCall;
use serde_json::json;

use super::ActiveReasoning;
use super::ApplyPatchState;
use super::OngoingToolCall;
use super::ResponsesStreamState;
use super::SummaryState;
use super::content_to_parts;
use super::provider_call;
use super::provider_result;
use super::tool_input_start;
use crate::responses::api_types::OutputItem;
use crate::responses::output::map_file_search_output;
use crate::responses::output::map_web_search_output;
use crate::responses::output::metadata;
use crate::stream_util::escape_json_delta;

impl ResponsesStreamState {
    pub(super) fn item_added(
        &mut self,
        item: &OutputItem,
        output_index: u64,
        parts: &mut Vec<StreamPart>,
    ) {
        let id = item.id_str().to_owned();
        match item.kind.as_str() {
            "function_call" => {
                if item.get_str("name") == Some("parallel")
                    && !self.mapper.function_names.contains("parallel")
                {
                    return;
                }
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let name = self.custom_name(item.get_str("name").unwrap_or_default());
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: call_id.clone(),
                        container_id: None,
                        apply_patch: None,
                        is_async: item.get("async").filter(|v| !v.is_null()).cloned(),
                    },
                );
                parts.push(tool_input_start(&call_id, &name, false));
            }
            "custom_tool_call" => {
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let name = self.custom_name(item.get_str("name").unwrap_or_default());
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: call_id.clone(),
                        container_id: None,
                        apply_patch: None,
                        is_async: item.get("async").filter(|v| !v.is_null()).cloned(),
                    },
                );
                parts.push(tool_input_start(&call_id, &name, false));
            }
            "web_search_call" => {
                let name = self.mapper.web_search_name();
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: id.clone(),
                        container_id: None,
                        apply_patch: None,
                        is_async: None,
                    },
                );
                parts.push(tool_input_start(&id, &name, true));
                parts.push(StreamPart::ToolInputEnd {
                    id: id.as_str().into(),
                    provider_metadata: None,
                });
                parts.push(StreamPart::ToolCall(provider_call(&id, &name, "{}")));
            }
            "computer_call" => {
                let call_id = item.get_str("call_id").unwrap_or(&id).to_owned();
                let name = self.custom_name("computer");
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: call_id.clone(),
                        container_id: None,
                        apply_patch: None,
                        is_async: None,
                    },
                );
                parts.push(tool_input_start(&call_id, &name, false));
            }
            "code_interpreter_call" => {
                let name = self.custom_name("code_interpreter");
                let container_id = item.get_str("container_id").unwrap_or_default().to_owned();
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: id.clone(),
                        container_id: Some(container_id.clone()),
                        apply_patch: None,
                        is_async: None,
                    },
                );
                parts.push(tool_input_start(&id, &name, true));
                parts.push(StreamPart::ToolInputDelta {
                    id: id.as_str().into(),
                    delta: format!(
                        "{{\"containerId\":\"{}\",\"code\":\"",
                        escape_json_delta(&container_id)
                    ),
                    provider_metadata: None,
                });
            }
            "file_search_call" => {
                let name = self.custom_name("file_search");
                parts.push(StreamPart::ToolCall(provider_call(&id, &name, "{}")));
            }
            "image_generation_call" => {
                let name = self.custom_name("image_generation");
                parts.push(StreamPart::ToolCall(provider_call(&id, &name, "{}")));
            }
            "apply_patch_call" => {
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let name = self.custom_name("apply_patch");
                let operation = item.get("operation").cloned().unwrap_or(JsonValue::Null);
                let op_type = operation
                    .get("type")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let is_delete = op_type == "delete_file";
                self.ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_call_id: call_id.clone(),
                        container_id: None,
                        apply_patch: Some(ApplyPatchState {
                            has_diff: is_delete,
                            end_emitted: is_delete,
                        }),
                        is_async: None,
                    },
                );
                parts.push(tool_input_start(&call_id, &name, false));
                if is_delete {
                    let input = json!({"callId": call_id, "operation": operation}).to_string();
                    parts.push(StreamPart::ToolInputDelta {
                        id: call_id.as_str().into(),
                        delta: input,
                        provider_metadata: None,
                    });
                    parts.push(StreamPart::ToolInputEnd {
                        id: call_id.as_str().into(),
                        provider_metadata: None,
                    });
                } else {
                    let path = operation
                        .get("path")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default();
                    parts.push(StreamPart::ToolInputDelta {
                        id: call_id.as_str().into(),
                        delta: format!(
                            "{{\"callId\":\"{}\",\"operation\":{{\"type\":\"{}\",\"path\":\"{}\",\"diff\":\"",
                            escape_json_delta(&call_id),
                            escape_json_delta(&op_type),
                            escape_json_delta(path)
                        ),
                        provider_metadata: None,
                    });
                }
            }
            "message" => {
                self.active_item_ids.insert(output_index, id.clone());
                self.ongoing_annotations.clear();
                self.active_message_phase = item.get("phase").filter(|p| !p.is_null()).cloned();
                let mut meta = self.item_id_meta(&id);
                if let Some(phase) = &self.active_message_phase {
                    meta.insert("phase".to_owned(), phase.clone());
                }
                parts.push(StreamPart::TextStart {
                    id: id.as_str().into(),
                    provider_metadata: Some(metadata(self.key(), meta)),
                });
            }
            "reasoning" => {
                self.active_item_ids.insert(output_index, id.clone());
                let encrypted = item
                    .get("encrypted_content")
                    .cloned()
                    .unwrap_or(JsonValue::Null);
                let mut summaries = BTreeMap::new();
                summaries.insert(0, SummaryState::Active);
                self.active_reasoning.insert(
                    id.clone(),
                    ActiveReasoning {
                        encrypted_content: encrypted.clone(),
                        summaries,
                    },
                );
                let mut meta = self.item_id_meta(&id);
                meta.insert("reasoningEncryptedContent".to_owned(), encrypted);
                parts.push(StreamPart::ReasoningStart {
                    id: format!("{id}:0").into(),
                    provider_metadata: Some(metadata(self.key(), meta)),
                });
            }
            _ => {}
        }
    }

    pub(super) fn item_done(
        &mut self,
        item: &OutputItem,
        output_index: u64,
        parts: &mut Vec<StreamPart>,
    ) {
        let id = item.id_str().to_owned();
        match item.kind.as_str() {
            "message" => {
                let item_id = self.resolve_item_id(&id, Some(output_index));
                let phase = item
                    .get("phase")
                    .filter(|p| !p.is_null())
                    .cloned()
                    .or_else(|| self.active_message_phase.take());
                self.active_message_phase = None;
                let mut meta = self.item_id_meta(&item_id);
                if let Some(phase) = phase {
                    meta.insert("phase".to_owned(), phase);
                }
                if !self.ongoing_annotations.is_empty() {
                    meta.insert(
                        "annotations".to_owned(),
                        JsonValue::Array(std::mem::take(&mut self.ongoing_annotations)),
                    );
                }
                parts.push(StreamPart::TextEnd {
                    id: item_id.into(),
                    provider_metadata: Some(metadata(self.key(), meta)),
                });
                self.active_item_ids.remove(&output_index);
            }
            "function_call" => {
                if let Some(calls) = crate::responses::parallel::expand(
                    item,
                    &self.mapper.function_names,
                    self.key(),
                ) {
                    self.mapper.has_function_call = true;
                    for content in calls {
                        if let Content::ToolCall(call) = content {
                            parts.push(tool_input_start(
                                call.tool_call_id.as_str(),
                                call.tool_name.as_str(),
                                false,
                            ));
                            parts.push(StreamPart::ToolInputDelta {
                                id: call.tool_call_id.as_str().into(),
                                delta: call.input.clone(),
                                provider_metadata: None,
                            });
                            parts.push(StreamPart::ToolInputEnd {
                                id: call.tool_call_id.as_str().into(),
                                provider_metadata: None,
                            });
                            parts.push(StreamPart::ToolCall(call));
                        }
                    }
                    return;
                }
                let ongoing = self.ongoing_tool_calls.remove(&output_index);
                self.mapper.has_function_call = true;
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let name = self.custom_name(item.get_str("name").unwrap_or_default());
                let arguments = item.get_str("arguments").unwrap_or("{}").to_owned();
                if ongoing.is_none() {
                    parts.push(tool_input_start(&call_id, &name, false));
                    parts.push(StreamPart::ToolInputDelta {
                        id: call_id.as_str().into(),
                        delta: arguments.clone(),
                        provider_metadata: None,
                    });
                }
                let namespace = item.get("namespace").filter(|v| !v.is_null()).cloned();
                let end_meta = namespace.as_ref().map(|namespace| {
                    let mut meta = JsonObject::new();
                    meta.insert("namespace".to_owned(), namespace.clone());
                    metadata(self.key(), meta)
                });
                parts.push(StreamPart::ToolInputEnd {
                    id: call_id.as_str().into(),
                    provider_metadata: end_meta,
                });
                let mut meta = self.item_id_meta(&id);
                let is_async = item
                    .get("async")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .or_else(|| ongoing.and_then(|o| o.is_async));
                if let Some(is_async) = is_async {
                    meta.insert("async".to_owned(), is_async);
                }
                if let Some(namespace) = namespace {
                    meta.insert("namespace".to_owned(), namespace);
                }
                if let Some(caller) = item.get("caller").filter(|v| !v.is_null()) {
                    let mapped =
                        if caller.get("type").and_then(JsonValue::as_str) == Some("program") {
                            json!({"type": "program", "callerId": caller.get("caller_id")})
                        } else {
                            caller.clone()
                        };
                    meta.insert("caller".to_owned(), mapped);
                }
                let mut call = ToolCall::new(call_id, name, arguments);
                call.provider_metadata = Some(metadata(self.key(), meta));
                parts.push(StreamPart::ToolCall(call));
            }
            "custom_tool_call" => {
                let ongoing = self.ongoing_tool_calls.remove(&output_index);
                self.mapper.has_function_call = true;
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let name = self.custom_name(item.get_str("name").unwrap_or_default());
                if ongoing.is_none() {
                    parts.push(tool_input_start(&call_id, &name, false));
                }
                parts.push(StreamPart::ToolInputEnd {
                    id: call_id.as_str().into(),
                    provider_metadata: None,
                });
                let mut meta = self.item_id_meta(&id);
                if let Some(is_async) = item
                    .get("async")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .or_else(|| ongoing.and_then(|o| o.is_async))
                {
                    meta.insert("async".to_owned(), is_async);
                }
                let input = item.get("input").cloned().unwrap_or(JsonValue::Null);
                let mut call = ToolCall::new(call_id, name, input.to_string());
                call.provider_metadata = Some(metadata(self.key(), meta));
                parts.push(StreamPart::ToolCall(call));
            }
            "web_search_call" => {
                self.ongoing_tool_calls.remove(&output_index);
                let name = self.mapper.web_search_name();
                parts.push(StreamPart::ToolResult(provider_result(
                    &id,
                    &name,
                    map_web_search_output(item.get("action")),
                )));
            }
            "computer_call" => {
                let ongoing = self.ongoing_tool_calls.remove(&output_index);
                let name = self.custom_name("computer");
                let call_id = item.get_str("call_id").unwrap_or(&id).to_owned();
                if ongoing.is_none() {
                    parts.push(tool_input_start(&call_id, &name, false));
                }
                self.mapper.has_function_call = true;
                let input = json!({
                    "action": item.get("action"),
                    "pendingSafetyChecks": item.get("pending_safety_checks"),
                })
                .to_string();
                parts.push(StreamPart::ToolInputDelta {
                    id: call_id.as_str().into(),
                    delta: input.clone(),
                    provider_metadata: None,
                });
                parts.push(StreamPart::ToolInputEnd {
                    id: call_id.as_str().into(),
                    provider_metadata: None,
                });
                let mut call = ToolCall::new(call_id, name, input);
                call.provider_metadata = Some(metadata(self.key(), self.item_id_meta(&id)));
                parts.push(StreamPart::ToolCall(call));
            }
            "file_search_call" => {
                self.ongoing_tool_calls.remove(&output_index);
                let name = self.custom_name("file_search");
                parts.push(StreamPart::ToolResult(provider_result(
                    &id,
                    &name,
                    map_file_search_output(item),
                )));
            }
            "code_interpreter_call" => {
                let ongoing = self.ongoing_tool_calls.remove(&output_index);
                let name = self.custom_name("code_interpreter");
                if ongoing.is_none() {
                    let input =
                        json!({"code": item.get("code"), "containerId": item.get("container_id")});
                    parts.push(StreamPart::ToolCall(provider_call(
                        &id,
                        &name,
                        &input.to_string(),
                    )));
                }
                parts.push(StreamPart::ToolResult(provider_result(
                    &id,
                    &name,
                    json!({"outputs": item.get("outputs")}),
                )));
            }
            "image_generation_call" => {
                let name = self.custom_name("image_generation");
                parts.push(StreamPart::ToolResult(provider_result(
                    &id,
                    &name,
                    json!({"result": item.get("result")}),
                )));
            }
            "mcp_call" => {
                self.ongoing_tool_calls.remove(&output_index);
                let approval = item.get_str("approval_request_id").map(str::to_owned);
                let alias = approval.as_deref().and_then(|approval| {
                    self.approval_ids_from_stream
                        .get(approval)
                        .or_else(|| self.mapper.approval_tool_call_ids.get(approval))
                        .cloned()
                });
                let mut mapped = self.mapper.map_item(item, false);
                if let Some(alias) = alias {
                    for part in &mut mapped {
                        match part {
                            Content::ToolCall(call) => call.tool_call_id = alias.as_str().into(),
                            Content::ToolResult(result) => {
                                result.tool_call_id = alias.as_str().into()
                            }
                            _ => {}
                        }
                    }
                }
                parts.extend(content_to_parts(mapped));
            }
            "mcp_approval_request" => {
                self.ongoing_tool_calls.remove(&output_index);
                let mapped = self.mapper.map_item(item, false);
                for part in &mapped {
                    if let Content::ToolApprovalRequest {
                        approval_id,
                        tool_call_id,
                        ..
                    } = part
                    {
                        self.approval_ids_from_stream.insert(
                            approval_id.as_str().to_owned(),
                            tool_call_id.as_str().to_owned(),
                        );
                    }
                }
                parts.extend(content_to_parts(mapped));
            }
            "apply_patch_call" => {
                let operation = item.get("operation").cloned().unwrap_or(JsonValue::Null);
                let op_type = operation
                    .get("type")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                if let Some(call) = self.ongoing_tool_calls.get_mut(&output_index) {
                    let call_id = call.tool_call_id.clone();
                    if let Some(state) = call.apply_patch.as_mut()
                        && !state.end_emitted
                        && op_type != "delete_file"
                    {
                        if !state.has_diff {
                            parts.push(StreamPart::ToolInputDelta {
                                id: call_id.as_str().into(),
                                delta: escape_json_delta(
                                    operation
                                        .get("diff")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default(),
                                ),
                                provider_metadata: None,
                            });
                        }
                        parts.push(StreamPart::ToolInputDelta {
                            id: call_id.as_str().into(),
                            delta: "\"}}".to_owned(),
                            provider_metadata: None,
                        });
                        parts.push(StreamPart::ToolInputEnd {
                            id: call_id.as_str().into(),
                            provider_metadata: None,
                        });
                        state.end_emitted = true;
                    }
                    if item.status.as_deref() == Some("completed") {
                        self.mapper.has_function_call = true;
                        let input = json!({"callId": item.get("call_id"), "operation": operation})
                            .to_string();
                        let mut tool_call =
                            ToolCall::new(call_id, self.custom_name("apply_patch"), input);
                        tool_call.provider_metadata =
                            Some(metadata(self.key(), self.item_id_meta(&id)));
                        parts.push(StreamPart::ToolCall(tool_call));
                    }
                }
                self.ongoing_tool_calls.remove(&output_index);
            }
            "local_shell_call" | "shell_call" | "program" | "tool_search_call" => {
                self.ongoing_tool_calls.remove(&output_index);
                let mapped = self.mapper.map_item(item, false);
                for content in mapped {
                    if let Content::ToolCall(call) = content {
                        let id = call.tool_call_id.as_str().to_owned();
                        parts.push(tool_input_start(
                            &id,
                            call.tool_name.as_str(),
                            call.provider_executed,
                        ));
                        parts.push(StreamPart::ToolInputDelta {
                            id: id.as_str().into(),
                            delta: call.input.clone(),
                            provider_metadata: None,
                        });
                        parts.push(StreamPart::ToolInputEnd {
                            id: id.as_str().into(),
                            provider_metadata: None,
                        });
                        parts.push(StreamPart::ToolCall(call));
                    }
                }
            }
            "shell_call_output" | "program_output" | "tool_search_output" => {
                parts.extend(content_to_parts(self.mapper.map_item(item, false)));
            }
            "reasoning" => {
                let item_id = self.resolve_item_id(&id, Some(output_index));
                if let Some(active) = self.active_reasoning.remove(&item_id) {
                    let encrypted = item
                        .get("encrypted_content")
                        .cloned()
                        .unwrap_or(JsonValue::Null);
                    for (index, state) in &active.summaries {
                        if matches!(state, SummaryState::Active | SummaryState::CanConclude) {
                            let mut meta = self.item_id_meta(&item_id);
                            meta.insert("reasoningEncryptedContent".to_owned(), encrypted.clone());
                            parts.push(StreamPart::ReasoningEnd {
                                id: format!("{item_id}:{index}").into(),
                                provider_metadata: Some(metadata(self.key(), meta)),
                            });
                        }
                    }
                }
                self.active_item_ids.remove(&output_index);
            }
            "compaction" => {
                parts.extend(content_to_parts(self.mapper.map_item(item, false)));
            }
            _ => {}
        }
    }
}
