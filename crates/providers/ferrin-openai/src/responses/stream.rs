//! Streaming state machine of the Responses API.

use std::collections::BTreeMap;
use std::collections::HashMap;

use ferrin_provider_util::ParseResult;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Usage;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::FinishReason;
use ferrin_spec::language_model::FinishReasonKind;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::ToolCall;
use serde_json::json;

use super::api_types::ResponsesChunk;
use super::api_types::ResponsesUsage;
use super::output::OutputMapper;
use super::output::annotation_source;
use super::output::map_finish_reason;
use super::output::map_usage;
use super::output::metadata;
use crate::error::stream_error_for_frame;
use crate::stream_util::StreamMachine;
use crate::stream_util::escape_json_delta;
use crate::stream_util::timestamp_from_seconds;

mod items;

/// Converts content parts produced by the shared mapper to stream parts.
fn content_to_parts(content: Vec<Content>) -> Vec<StreamPart> {
    content
        .into_iter()
        .filter_map(|part| match part {
            Content::ToolCall(call) => Some(StreamPart::ToolCall(call)),
            Content::ToolResult(result) => Some(StreamPart::ToolResult(result)),
            Content::ToolApprovalRequest {
                approval_id,
                tool_call_id,
                provider_metadata,
            } => Some(StreamPart::ToolApprovalRequest {
                approval_id,
                tool_call_id,
                provider_metadata,
            }),
            Content::Source(source) => Some(StreamPart::Source(source)),
            Content::Custom {
                kind,
                provider_metadata,
            } => Some(StreamPart::Custom {
                kind,
                provider_metadata,
            }),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryState {
    Active,
    CanConclude,
    Concluded,
}

#[derive(Debug)]
struct ActiveReasoning {
    encrypted_content: JsonValue,
    summaries: BTreeMap<u64, SummaryState>,
}

#[derive(Debug)]
struct OngoingToolCall {
    tool_call_id: String,
    container_id: Option<String>,
    apply_patch: Option<ApplyPatchState>,
    is_async: Option<JsonValue>,
}

#[derive(Debug, Default)]
struct ApplyPatchState {
    has_diff: bool,
    end_emitted: bool,
}

/// State of one streamed response.
pub struct ResponsesStreamState {
    mapper: OutputMapper,
    store: bool,
    collect_logprobs: bool,
    finish_reason: FinishReason,
    usage: Option<ResponsesUsage>,
    raw_usage: Option<JsonObject>,
    response_id: Option<String>,
    service_tier: Option<String>,
    reasoning_context: Option<JsonValue>,
    ongoing_tool_calls: HashMap<u64, OngoingToolCall>,
    ongoing_annotations: Vec<JsonValue>,
    active_message_phase: Option<JsonValue>,
    active_reasoning: HashMap<String, ActiveReasoning>,
    active_item_ids: HashMap<u64, String>,
    approval_ids_from_stream: HashMap<String, String>,
    encountered_error: bool,
}

impl std::fmt::Debug for ResponsesStreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponsesStreamState")
            .field("response_id", &self.response_id)
            .field("finish_reason", &self.finish_reason)
            .finish_non_exhaustive()
    }
}

impl ResponsesStreamState {
    /// Creates the state for one stream.
    #[must_use]
    pub fn new(mapper: OutputMapper, store: bool, collect_logprobs: bool) -> Self {
        Self {
            mapper,
            store,
            collect_logprobs,
            finish_reason: FinishReason::new(FinishReasonKind::Other),
            usage: None,
            raw_usage: None,
            response_id: None,
            service_tier: None,
            reasoning_context: None,
            ongoing_tool_calls: HashMap::new(),
            ongoing_annotations: Vec::new(),
            active_message_phase: None,
            active_reasoning: HashMap::new(),
            active_item_ids: HashMap::new(),
            approval_ids_from_stream: HashMap::new(),
            encountered_error: false,
        }
    }

    fn key(&self) -> &str {
        &self.mapper.config.provider_options_key
    }

    fn custom_name(&self, provider_name: &str) -> String {
        self.mapper
            .tool_name_mapping
            .to_custom_tool_name(provider_name)
            .to_owned()
    }

    fn item_id_meta(&self, item_id: &str) -> JsonObject {
        let mut meta = JsonObject::new();
        meta.insert("itemId".to_owned(), JsonValue::from(item_id));
        meta
    }

    fn resolve_item_id(&self, item_id: &str, output_index: Option<u64>) -> String {
        output_index
            .and_then(|index| self.active_item_ids.get(&index))
            .cloned()
            .unwrap_or_else(|| item_id.to_owned())
    }

    /// Handles one parsed chunk (or parse failure).
    pub fn handle_parsed(
        &mut self,
        chunk: ParseResult<ResponsesChunk>,
        include_raw: bool,
    ) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        match chunk {
            ParseResult::Ok { value, raw } => {
                if include_raw {
                    parts.push(StreamPart::Raw {
                        raw_value: raw.clone(),
                    });
                }
                self.handle_chunk(&value, &raw, &mut parts);
            }
            ParseResult::Err { error, raw } => {
                if include_raw && let Some(raw) = raw {
                    parts.push(StreamPart::Raw {
                        raw_value: JsonValue::from(raw),
                    });
                }
                self.encountered_error = true;
                self.finish_reason = FinishReason::error();
                parts.push(StreamPart::error(&error));
            }
        }
        parts
    }

    fn handle_chunk(
        &mut self,
        chunk: &ResponsesChunk,
        raw: &JsonValue,
        parts: &mut Vec<StreamPart>,
    ) {
        let output_index = chunk.get_u64("output_index");
        match chunk.kind.as_str() {
            "response.output_item.added" => {
                if let Some(item) = chunk.item() {
                    self.item_added(&item, output_index.unwrap_or_default(), parts);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = chunk.item() {
                    self.item_done(&item, output_index.unwrap_or_default(), parts);
                }
            }
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                if let Some(call) = output_index.and_then(|i| self.ongoing_tool_calls.get(&i))
                    && let Some(delta) = chunk.get_str("delta")
                {
                    parts.push(StreamPart::ToolInputDelta {
                        id: call.tool_call_id.as_str().into(),
                        delta: delta.to_owned(),
                        provider_metadata: None,
                    });
                }
            }
            "response.apply_patch_call_operation_diff.delta" => {
                if let Some(call) = output_index.and_then(|i| self.ongoing_tool_calls.get_mut(&i))
                    && let Some(state) = call.apply_patch.as_mut()
                    && let Some(delta) = chunk.get_str("delta")
                {
                    parts.push(StreamPart::ToolInputDelta {
                        id: call.tool_call_id.as_str().into(),
                        delta: escape_json_delta(delta),
                        provider_metadata: None,
                    });
                    state.has_diff = true;
                }
            }
            "response.apply_patch_call_operation_diff.done" => {
                if let Some(call) = output_index.and_then(|i| self.ongoing_tool_calls.get_mut(&i))
                    && let Some(state) = call.apply_patch.as_mut()
                    && !state.end_emitted
                {
                    let id = call.tool_call_id.as_str();
                    if !state.has_diff {
                        parts.push(StreamPart::ToolInputDelta {
                            id: id.into(),
                            delta: escape_json_delta(chunk.get_str("diff").unwrap_or_default()),
                            provider_metadata: None,
                        });
                        state.has_diff = true;
                    }
                    parts.push(StreamPart::ToolInputDelta {
                        id: id.into(),
                        delta: "\"}}".to_owned(),
                        provider_metadata: None,
                    });
                    parts.push(StreamPart::ToolInputEnd {
                        id: id.into(),
                        provider_metadata: None,
                    });
                    state.end_emitted = true;
                }
            }
            "response.image_generation_call.partial_image" => {
                let name = self.custom_name("image_generation");
                parts.push(StreamPart::ToolResult(ProviderToolResult {
                    tool_call_id: chunk.get_str("item_id").unwrap_or_default().into(),
                    tool_name: name.into(),
                    result: json!({"result": chunk.get("partial_image_b64")}),
                    is_error: false,
                    preliminary: true,
                    dynamic: false,
                    provider_metadata: None,
                }));
            }
            "response.code_interpreter_call_code.delta" => {
                if let Some(call) = output_index.and_then(|i| self.ongoing_tool_calls.get(&i))
                    && let Some(delta) = chunk.get_str("delta")
                {
                    parts.push(StreamPart::ToolInputDelta {
                        id: call.tool_call_id.as_str().into(),
                        delta: escape_json_delta(delta),
                        provider_metadata: None,
                    });
                }
            }
            "response.code_interpreter_call_code.done" => {
                if let Some(call) = output_index.and_then(|i| self.ongoing_tool_calls.get(&i)) {
                    let id = call.tool_call_id.clone();
                    parts.push(StreamPart::ToolInputDelta {
                        id: id.as_str().into(),
                        delta: "\"}".to_owned(),
                        provider_metadata: None,
                    });
                    parts.push(StreamPart::ToolInputEnd {
                        id: id.as_str().into(),
                        provider_metadata: None,
                    });
                    let input = json!({
                        "code": chunk.get("code"),
                        "containerId": call.container_id,
                    });
                    let name = self.custom_name("code_interpreter");
                    parts.push(StreamPart::ToolCall(provider_call(
                        &id,
                        &name,
                        &input.to_string(),
                    )));
                }
            }
            "response.created" => {
                if let Some(response) = chunk.response() {
                    self.response_id.clone_from(&response.id);
                    parts.push(StreamPart::ResponseMetadata {
                        id: response.id,
                        timestamp: timestamp_from_seconds(response.created_at),
                        model_id: response.model.map(Into::into),
                    });
                }
            }
            "response.output_text.delta" => {
                let item_id = self
                    .resolve_item_id(chunk.get_str("item_id").unwrap_or_default(), output_index);
                parts.push(StreamPart::TextDelta {
                    id: item_id.into(),
                    delta: chunk.get_str("delta").unwrap_or_default().to_owned(),
                    provider_metadata: None,
                });
                if self.collect_logprobs
                    && let Some(logprobs) = chunk.get("logprobs")
                {
                    self.mapper.logprobs.push(logprobs.clone());
                }
            }
            "response.reasoning_summary_part.added" => {
                let item_id = self
                    .resolve_item_id(chunk.get_str("item_id").unwrap_or_default(), output_index);
                let summary_index = chunk.get_u64("summary_index").unwrap_or(0);
                if summary_index > 0 {
                    let key = self.key().to_owned();
                    if let Some(active) = self.active_reasoning.get_mut(&item_id) {
                        active.summaries.insert(summary_index, SummaryState::Active);
                        for (index, state) in &mut active.summaries {
                            if *state == SummaryState::CanConclude {
                                let mut meta = JsonObject::new();
                                meta.insert("itemId".to_owned(), JsonValue::from(item_id.as_str()));
                                parts.push(StreamPart::ReasoningEnd {
                                    id: format!("{item_id}:{index}").into(),
                                    provider_metadata: Some(metadata(&key, meta)),
                                });
                                *state = SummaryState::Concluded;
                            }
                        }
                        let mut meta = JsonObject::new();
                        meta.insert("itemId".to_owned(), JsonValue::from(item_id.as_str()));
                        meta.insert(
                            "reasoningEncryptedContent".to_owned(),
                            active.encrypted_content.clone(),
                        );
                        parts.push(StreamPart::ReasoningStart {
                            id: format!("{item_id}:{summary_index}").into(),
                            provider_metadata: Some(metadata(&key, meta)),
                        });
                    }
                }
            }
            "response.reasoning_summary_text.delta" => {
                let item_id = self
                    .resolve_item_id(chunk.get_str("item_id").unwrap_or_default(), output_index);
                let summary_index = chunk.get_u64("summary_index").unwrap_or(0);
                let meta = self.item_id_meta(&item_id);
                parts.push(StreamPart::ReasoningDelta {
                    id: format!("{item_id}:{summary_index}").into(),
                    delta: chunk.get_str("delta").unwrap_or_default().to_owned(),
                    provider_metadata: Some(metadata(self.key(), meta)),
                });
            }
            "response.reasoning_summary_part.done" => {
                let item_id = self
                    .resolve_item_id(chunk.get_str("item_id").unwrap_or_default(), output_index);
                let summary_index = chunk.get_u64("summary_index").unwrap_or(0);
                let key = self.key().to_owned();
                let store = self.store;
                if let Some(active) = self.active_reasoning.get_mut(&item_id) {
                    if store {
                        let mut meta = JsonObject::new();
                        meta.insert("itemId".to_owned(), JsonValue::from(item_id.as_str()));
                        parts.push(StreamPart::ReasoningEnd {
                            id: format!("{item_id}:{summary_index}").into(),
                            provider_metadata: Some(metadata(&key, meta)),
                        });
                        active
                            .summaries
                            .insert(summary_index, SummaryState::Concluded);
                    } else {
                        active
                            .summaries
                            .insert(summary_index, SummaryState::CanConclude);
                    }
                }
            }
            "response.completed" | "response.incomplete" => {
                if let Some(response) = chunk.response() {
                    if !self.encountered_error {
                        let reason = response
                            .incomplete_details
                            .as_ref()
                            .and_then(|d| d.reason.as_deref());
                        self.finish_reason =
                            map_finish_reason(reason, self.mapper.has_function_call);
                    }
                    self.usage = response.usage;
                    self.raw_usage = raw
                        .get("response")
                        .and_then(|r| r.get("usage"))
                        .and_then(JsonValue::as_object)
                        .cloned();
                    if response.service_tier.is_some() {
                        self.service_tier = response.service_tier;
                    }
                    if let Some(context) = response.reasoning.and_then(|r| r.context) {
                        self.reasoning_context = Some(context);
                    }
                }
            }
            "response.failed" => {
                if let Some(response) = chunk.response() {
                    let reason = response
                        .incomplete_details
                        .as_ref()
                        .and_then(|d| d.reason.clone());
                    self.finish_reason = match &reason {
                        Some(reason) => {
                            map_finish_reason(Some(reason), self.mapper.has_function_call)
                        }
                        None => FinishReason::with_raw(FinishReasonKind::Error, "error"),
                    };
                    self.usage = response.usage;
                    if let Some(context) = response.reasoning.and_then(|r| r.context) {
                        self.reasoning_context = Some(context);
                    }
                    if !self.encountered_error && response.error.is_some() {
                        self.encountered_error = true;
                        parts.push(StreamPart::Error {
                            error: stream_error_for_frame(raw),
                        });
                    }
                }
            }
            "response.output_text.annotation.added" => {
                if let Some(annotation) = chunk.get("annotation") {
                    self.ongoing_annotations.push(annotation.clone());
                    parts.extend(content_to_parts(
                        annotation_source(annotation, &self.mapper.config)
                            .into_iter()
                            .collect(),
                    ));
                }
            }
            "error" => {
                self.encountered_error = true;
                self.finish_reason = FinishReason::with_raw(FinishReasonKind::Error, "error");
                parts.push(StreamPart::Error {
                    error: stream_error_for_frame(raw),
                });
            }
            _ => {}
        }
    }

    /// Emits the closing parts of the stream.
    #[must_use]
    pub fn finish_parts(self) -> Vec<StreamPart> {
        let usage = self.usage.as_ref().map_or_else(Usage::default, |usage| {
            map_usage(usage, self.raw_usage.clone())
        });
        vec![StreamPart::Finish {
            finish_reason: self.finish_reason,
            usage,
            provider_metadata: Some(self.mapper.response_metadata(
                self.response_id.as_deref(),
                self.service_tier.as_deref(),
                self.reasoning_context.as_ref(),
            )),
        }]
    }
}

impl StreamMachine for ResponsesStreamState {
    type Chunk = ResponsesChunk;

    fn handle(&mut self, chunk: ParseResult<ResponsesChunk>, include_raw: bool) -> Vec<StreamPart> {
        self.handle_parsed(chunk, include_raw)
    }

    fn finish(self) -> Vec<StreamPart> {
        self.finish_parts()
    }
}

fn tool_input_start(id: &str, name: &str, provider_executed: bool) -> StreamPart {
    StreamPart::ToolInputStart {
        id: id.into(),
        tool_name: name.into(),
        provider_executed,
        dynamic: false,
        title: None,
        provider_metadata: None,
    }
}

fn provider_call(id: &str, name: &str, input: &str) -> ToolCall {
    let mut call = ToolCall::new(id, name, input);
    call.provider_executed = true;
    call
}

fn provider_result(id: &str, name: &str, result: JsonValue) -> ProviderToolResult {
    ProviderToolResult {
        tool_call_id: id.into(),
        tool_name: name.into(),
        result,
        is_error: false,
        preliminary: false,
        dynamic: false,
        provider_metadata: None,
    }
}
