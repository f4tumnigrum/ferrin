//! Mapping of provider stream parts to stream events and attempt state.

use crate::generate_text::tools::ToolEnvironment;
use std::collections::HashSet;

use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::StreamPart;

use super::attempt::Attempt;
use super::attempt::ToolInputInfo;
use crate::error::Error;
use crate::generate_text::GeneratedFile;
use crate::generate_text::StepContent;
use crate::generate_text::ToolApprovalRequestContent;
use crate::generate_text::ToolErrorInfo;
use crate::generate_text::ToolExecutionError;
use crate::generate_text::ToolResult;
use crate::generate_text::parse_tool_call::ParseContext;
use crate::generate_text::parse_tool_call::parse_tool_call;
use crate::generate_text::tools::resolve_call_approval;
use crate::stream_text::StreamEvent;

/// Reserves a call-unique part id, suffixing `-<n>` on conflicts.
fn reserve_part_id(used: &mut HashSet<PartId>, id: &PartId) -> PartId {
    if used.insert(id.clone()) {
        return id.clone();
    }
    let mut n: u32 = 1;
    loop {
        let candidate = PartId::new(format!("{id}-{n}"));
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

impl Attempt {
    /// Maps one part (other than stream-start, finish and error) to events,
    /// updating the attempt state.
    pub(super) async fn map_part(
        &mut self,
        part: StreamPart,
        used_ids: &mut HashSet<PartId>,
    ) -> Result<Vec<StreamEvent>, Error> {
        match part {
            StreamPart::ResponseMetadata {
                id,
                timestamp,
                model_id,
            } => {
                if id.is_some() {
                    self.state.response.id = id;
                }
                if timestamp.is_some() {
                    self.state.response.timestamp = timestamp;
                }
                if model_id.is_some() {
                    self.state.response.model_id = model_id;
                }
                Ok(Vec::new())
            }
            StreamPart::TextStart {
                id,
                provider_metadata,
            } => {
                let mapped = self.start_part(false, id, provider_metadata.clone(), used_ids);
                Ok(vec![StreamEvent::TextStart {
                    id: mapped,
                    provider_metadata,
                }])
            }
            StreamPart::TextDelta {
                id,
                delta,
                provider_metadata,
            } => {
                let mapped =
                    self.update_part(false, &id, Some(&delta), provider_metadata.as_ref(), false)?;
                if delta.is_empty() && provider_metadata.is_none() {
                    return Ok(Vec::new());
                }
                Ok(vec![StreamEvent::TextDelta {
                    id: mapped,
                    text: delta,
                    provider_metadata,
                }])
            }
            StreamPart::TextEnd {
                id,
                provider_metadata,
            } => {
                let mapped =
                    self.update_part(false, &id, None, provider_metadata.as_ref(), true)?;
                Ok(vec![StreamEvent::TextEnd {
                    id: mapped,
                    provider_metadata,
                }])
            }
            StreamPart::ReasoningStart {
                id,
                provider_metadata,
            } => {
                let mapped = self.start_part(true, id, provider_metadata.clone(), used_ids);
                Ok(vec![StreamEvent::ReasoningStart {
                    id: mapped,
                    provider_metadata,
                }])
            }
            StreamPart::ReasoningDelta {
                id,
                delta,
                provider_metadata,
            } => {
                let mapped =
                    self.update_part(true, &id, Some(&delta), provider_metadata.as_ref(), false)?;
                if delta.is_empty() && provider_metadata.is_none() {
                    return Ok(Vec::new());
                }
                Ok(vec![StreamEvent::ReasoningDelta {
                    id: mapped,
                    text: delta,
                    provider_metadata,
                }])
            }
            StreamPart::ReasoningEnd {
                id,
                provider_metadata,
            } => {
                let mapped = self.update_part(true, &id, None, provider_metadata.as_ref(), true)?;
                Ok(vec![StreamEvent::ReasoningEnd {
                    id: mapped,
                    provider_metadata,
                }])
            }
            StreamPart::ToolInputStart {
                id,
                tool_name,
                provider_executed,
                dynamic,
                title,
                provider_metadata,
            } => {
                let tool = self.inputs.tools.get(tool_name.as_str()).cloned();
                let dynamic = dynamic || tool.as_ref().is_some_and(|tool| tool.kind().is_dynamic());
                let title = title.or_else(|| {
                    tool.as_ref()
                        .and_then(|tool| tool.title().map(str::to_owned))
                });
                if let Some(tool) = &tool
                    && let Some(hook) = &tool.hooks().on_input_start
                {
                    hook(self.tool_context_for(tool, &id, &tool_name)?).await;
                }
                self.state.tool_inputs.insert(
                    id.clone(),
                    ToolInputInfo {
                        tool,
                        tool_name: tool_name.clone(),
                    },
                );
                Ok(vec![StreamEvent::ToolInputStart {
                    id,
                    tool_name,
                    provider_executed,
                    dynamic,
                    title,
                    provider_metadata,
                }])
            }
            StreamPart::ToolInputDelta {
                id,
                delta,
                provider_metadata,
            } => {
                if let Some(info) = self.state.tool_inputs.get(&id)
                    && let Some(tool) = &info.tool
                    && let Some(hook) = &tool.hooks().on_input_delta
                {
                    hook(
                        delta.clone(),
                        self.tool_context_for(tool, &id, &info.tool_name)?,
                    )
                    .await;
                }
                Ok(vec![StreamEvent::ToolInputDelta {
                    id,
                    delta,
                    provider_metadata,
                }])
            }
            StreamPart::ToolInputEnd {
                id,
                provider_metadata,
            } => Ok(vec![StreamEvent::ToolInputEnd {
                id,
                provider_metadata,
            }]),
            StreamPart::ToolCall(call) => self.map_tool_call(&call).await,
            StreamPart::ToolResult(result) => {
                let tool_metadata = self
                    .state
                    .tool_calls
                    .iter()
                    .find(|call| call.tool_call_id == result.tool_call_id)
                    .and_then(|call| call.tool_metadata.clone())
                    .or_else(|| {
                        self.ctx
                            .execution_tools
                            .get(result.tool_name.as_str())
                            .and_then(|tool| tool.metadata().cloned())
                    });
                let input = self
                    .state
                    .tool_calls
                    .iter()
                    .find(|call| call.tool_call_id == result.tool_call_id)
                    .map_or(JsonValue::Null, |call| call.input.clone());
                let dynamic = result.dynamic
                    || self
                        .ctx
                        .execution_tools
                        .get(result.tool_name.as_str())
                        .is_some_and(|tool| tool.kind().is_dynamic());
                self.state.result_ids.insert(result.tool_call_id.clone());
                let event = if result.is_error {
                    let error = ToolExecutionError {
                        tool_call_id: result.tool_call_id,
                        tool_name: result.tool_name,
                        input,
                        error: ToolErrorInfo::Json {
                            value: result.result,
                        },
                        provider_executed: true,
                        dynamic,
                        tool_metadata,
                        provider_metadata: result.provider_metadata,
                    };
                    self.state
                        .content
                        .push(StepContent::ToolError(error.clone()));
                    StreamEvent::ToolError(error)
                } else {
                    let tool_result = ToolResult {
                        tool_call_id: result.tool_call_id,
                        tool_name: result.tool_name,
                        input,
                        output: result.result,
                        provider_executed: true,
                        dynamic,
                        preliminary: result.preliminary,
                        execution_ms: None,
                        tool_metadata,
                        provider_metadata: result.provider_metadata,
                    };
                    self.state
                        .content
                        .push(StepContent::ToolResult(tool_result.clone()));
                    StreamEvent::ToolResult(tool_result)
                };
                Ok(vec![event])
            }
            StreamPart::ToolApprovalRequest {
                approval_id,
                tool_call_id,
                provider_metadata,
            } => {
                let Some(call) = self
                    .state
                    .tool_calls
                    .iter()
                    .find(|call| call.tool_call_id == tool_call_id)
                    .cloned()
                else {
                    return Err(Error::ToolCallNotFoundForApproval {
                        tool_call_id,
                        approval_id,
                    });
                };
                let request = ToolApprovalRequestContent {
                    approval_id,
                    tool_call: call,
                    reason: None,
                    is_automatic: false,
                    signature: None,
                    provider_metadata,
                };
                self.state
                    .content
                    .push(StepContent::ToolApprovalRequest(request.clone()));
                Ok(vec![StreamEvent::ToolApprovalRequest(request)])
            }
            StreamPart::File {
                data,
                media_type,
                filename,
                provider_metadata,
            } => {
                let file = GeneratedFile {
                    data,
                    media_type,
                    filename,
                    provider_metadata,
                };
                self.state.content.push(StepContent::File(file.clone()));
                Ok(vec![StreamEvent::File(file)])
            }
            StreamPart::ReasoningFile {
                data,
                media_type,
                provider_metadata,
            } => {
                let file = GeneratedFile {
                    data,
                    media_type,
                    filename: None,
                    provider_metadata,
                };
                self.state
                    .content
                    .push(StepContent::ReasoningFile(file.clone()));
                Ok(vec![StreamEvent::ReasoningFile(file)])
            }
            StreamPart::Source(source) => {
                self.state.content.push(StepContent::Source(source.clone()));
                Ok(vec![StreamEvent::Source(source)])
            }
            StreamPart::Custom {
                kind,
                provider_metadata,
            } => {
                self.state.content.push(StepContent::Custom {
                    kind: kind.clone(),
                    provider_metadata: provider_metadata.clone(),
                });
                Ok(vec![StreamEvent::Custom {
                    kind,
                    provider_metadata,
                }])
            }
            StreamPart::Raw { raw_value } => Ok(if self.stream.include_raw_chunks {
                vec![StreamEvent::Raw { raw_value }]
            } else {
                Vec::new()
            }),
            // Stream start, finish and error parts are handled by the reader.
            _ => Ok(Vec::new()),
        }
    }

    /// Parses a tool call, runs the input hook, resolves approval and queues
    /// the call for execution when cleared.
    async fn map_tool_call(
        &mut self,
        call: &ferrin_spec::ToolCall,
    ) -> Result<Vec<StreamEvent>, Error> {
        self.inputs.refresh_tools(&self.ctx.model_tools);
        let parsed = {
            let parse_ctx = ParseContext {
                tools: &self.inputs.tools,
                tool_choice: self.inputs.tool_choice.as_ref(),
                repair: self.ctx.config.repair_tool_call.as_deref(),
                refine: &self.ctx.config.refine_tool_inputs,
                system: self.inputs.instructions.as_ref(),
                messages: &self.inputs.messages,
            };
            parse_tool_call(call, &parse_ctx).await
        };
        let mut events = vec![StreamEvent::ToolCall(parsed.clone())];
        self.state.tool_calls.push(parsed.clone());
        self.state
            .content
            .push(StepContent::ToolCall(parsed.clone()));
        if parsed.invalid {
            if !parsed.provider_executed {
                self.state.client_outputs += 1;
                events.push(StreamEvent::ToolError(ToolExecutionError {
                    tool_call_id: parsed.tool_call_id.clone(),
                    tool_name: parsed.tool_name.clone(),
                    input: parsed.input.clone(),
                    error: ToolErrorInfo::text(parsed.error.clone().unwrap_or_default()),
                    provider_executed: false,
                    dynamic: true,
                    tool_metadata: parsed.tool_metadata.clone(),
                    provider_metadata: parsed.provider_metadata.clone(),
                }));
            }
            return Ok(events);
        }

        let tool = self
            .ctx
            .execution_tools
            .get(parsed.tool_name.as_str())
            .cloned();
        if let Some(tool) = &tool
            && let Some(hook) = &tool.hooks().on_input_available
        {
            hook(
                parsed.input.clone(),
                self.tool_context_for(tool, &parsed.tool_call_id, &parsed.tool_name)?,
            )
            .await;
        }
        let executable =
            !parsed.provider_executed && tool.as_ref().is_some_and(|tool| tool.is_executable());
        match resolve_call_approval(
            &self.ctx,
            &parsed,
            &self.step_messages,
            ToolEnvironment::for_step(&self.inputs),
            &self.cancellation,
        )
        .await?
        {
            None => {
                if executable {
                    self.state.queued.push(parsed);
                }
            }
            Some(approval) => {
                events.push(StreamEvent::ToolApprovalRequest(approval.request));
                if let Some(response) = approval.response {
                    if !response.approved {
                        self.state.denied += 1;
                    }
                    events.push(StreamEvent::ToolApprovalResponse(response));
                }
                if !approval.blocked && executable {
                    self.state.queued.push(parsed);
                }
            }
        }
        Ok(events)
    }

    /// Records a new text or reasoning part and returns its reserved id.
    fn start_part(
        &mut self,
        reasoning: bool,
        id: PartId,
        provider_metadata: Option<ProviderMetadata>,
        used_ids: &mut HashSet<PartId>,
    ) -> PartId {
        let mapped = reserve_part_id(used_ids, &id);
        let index = self.state.content.len();
        self.state.content.push(if reasoning {
            StepContent::Reasoning {
                text: String::new(),
                provider_metadata,
            }
        } else {
            StepContent::Text {
                text: String::new(),
                provider_metadata,
            }
        });
        self.state.part_index.insert(mapped.clone(), index);
        let ids = if reasoning {
            &mut self.state.reasoning_ids
        } else {
            &mut self.state.text_ids
        };
        ids.insert(id, mapped.clone());
        mapped
    }

    /// Appends a delta or closes a part; returns the reserved id.
    fn update_part(
        &mut self,
        reasoning: bool,
        id: &PartId,
        delta: Option<&str>,
        provider_metadata: Option<&ProviderMetadata>,
        end: bool,
    ) -> Result<PartId, Error> {
        let kind = if reasoning { "reasoning" } else { "text" };
        let ids = if reasoning {
            &mut self.state.reasoning_ids
        } else {
            &mut self.state.text_ids
        };
        let mapped = if end {
            ids.remove(id)
        } else {
            ids.get(id).cloned()
        }
        .ok_or_else(|| Error::invalid_stream_part(format!("{kind} part `{id}` is not open")))?;
        if let Some(index) = self.state.part_index.get(&mapped).copied()
            && let Some(
                StepContent::Text {
                    text,
                    provider_metadata: metadata,
                }
                | StepContent::Reasoning {
                    text,
                    provider_metadata: metadata,
                },
            ) = self.state.content.get_mut(index)
        {
            if let Some(delta) = delta {
                text.push_str(delta);
            }
            if provider_metadata.is_some() {
                *metadata = provider_metadata.cloned();
            }
        }
        Ok(mapped)
    }
}
