//! Streaming state machine for `streamGenerateContent?alt=sse`.

use std::collections::HashSet;

use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolCallId;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::StreamPart;

use crate::api_types::FunctionCall;
use crate::api_types::GenerateContentResponse;
use crate::api_types::Part;
use crate::api_types::UsageMetadata;
use crate::json_accumulator::JsonAccumulator;
use crate::output::OutputMapper;
use crate::output::convert_usage;
use crate::output::map_finish_reason;

#[derive(Debug)]
struct ActiveToolCall {
    id: ToolCallId,
    tool_name: String,
    accumulator: JsonAccumulator,
    provider_metadata: Option<ProviderMetadata>,
}

/// Stream state: open text/reasoning blocks, streamed function calls, usage
/// and the metadata of the final `finish` part.
#[derive(Debug)]
pub struct GoogleStreamState {
    mapper: OutputMapper,
    finish_reason: FinishReason,
    received_finish_reason: bool,
    usage: Option<UsageMetadata>,
    raw_usage: Option<JsonObject>,
    provider_metadata: Option<ProviderMetadata>,
    last_grounding_metadata: Option<JsonValue>,
    last_url_context_metadata: Option<JsonValue>,
    has_tool_calls: bool,
    emitted_response_metadata: bool,
    text_block: Option<PartId>,
    reasoning_block: Option<PartId>,
    block_counter: u64,
    emitted_source_urls: HashSet<String>,
    active_calls: Vec<ActiveToolCall>,
}

impl GoogleStreamState {
    /// Creates the state for one stream.
    #[must_use]
    pub fn new(mapper: OutputMapper) -> Self {
        Self {
            mapper,
            finish_reason: FinishReason::new(FinishReasonKind::Other),
            received_finish_reason: false,
            usage: None,
            raw_usage: None,
            provider_metadata: None,
            last_grounding_metadata: None,
            last_url_context_metadata: None,
            has_tool_calls: false,
            emitted_response_metadata: false,
            text_block: None,
            reasoning_block: None,
            block_counter: 0,
            emitted_source_urls: HashSet::new(),
            active_calls: Vec::new(),
        }
    }

    fn next_block_id(&mut self) -> PartId {
        let id = PartId::new(self.block_counter.to_string());
        self.block_counter += 1;
        id
    }

    fn end_text(&mut self, parts: &mut Vec<StreamPart>) {
        if let Some(id) = self.text_block.take() {
            parts.push(StreamPart::TextEnd {
                id,
                provider_metadata: None,
            });
        }
    }

    fn end_reasoning(&mut self, parts: &mut Vec<StreamPart>) {
        if let Some(id) = self.reasoning_block.take() {
            parts.push(StreamPart::ReasoningEnd {
                id,
                provider_metadata: None,
            });
        }
    }

    fn finish_active_call(&mut self, parts: &mut Vec<StreamPart>) {
        let Some(active) = self.active_calls.pop() else {
            return;
        };
        let (final_json, closing_delta) = active.accumulator.finalize();
        if !closing_delta.is_empty() {
            parts.push(StreamPart::ToolInputDelta {
                id: active.id.clone(),
                delta: closing_delta,
                provider_metadata: active.provider_metadata.clone(),
            });
        }
        parts.push(StreamPart::ToolInputEnd {
            id: active.id.clone(),
            provider_metadata: active.provider_metadata.clone(),
        });
        let mut call = ToolCall::new(active.id, active.tool_name, final_json);
        call.provider_metadata = active.provider_metadata;
        parts.push(StreamPart::ToolCall(call));
        self.has_tool_calls = true;
    }

    fn finish_metadata(
        &self,
        response: &GenerateContentResponse,
        candidate_safety: Option<&JsonValue>,
        finish_message: Option<&str>,
    ) -> ProviderMetadata {
        let mut object = JsonObject::new();
        object.insert(
            "promptFeedback".to_owned(),
            response.prompt_feedback.clone().unwrap_or(JsonValue::Null),
        );
        object.insert(
            "groundingMetadata".to_owned(),
            self.last_grounding_metadata
                .clone()
                .unwrap_or(JsonValue::Null),
        );
        object.insert(
            "urlContextMetadata".to_owned(),
            self.last_url_context_metadata
                .clone()
                .unwrap_or(JsonValue::Null),
        );
        object.insert(
            "safetyRatings".to_owned(),
            candidate_safety.cloned().unwrap_or(JsonValue::Null),
        );
        object.insert(
            "usageMetadata".to_owned(),
            self.raw_usage
                .clone()
                .map_or(JsonValue::Null, JsonValue::Object),
        );
        object.insert(
            "finishMessage".to_owned(),
            finish_message.map_or(JsonValue::Null, JsonValue::from),
        );
        object.insert(
            "serviceTier".to_owned(),
            self.usage
                .as_ref()
                .and_then(|usage| usage.service_tier.clone())
                .map_or(JsonValue::Null, JsonValue::from),
        );
        self.mapper.metadata(object)
    }

    fn text_part(
        &mut self,
        text: &str,
        thought: bool,
        signature: Option<&str>,
        parts: &mut Vec<StreamPart>,
    ) {
        let metadata = self.mapper.signature_metadata(signature);
        if text.is_empty() {
            if let (Some(_), Some(id)) = (&metadata, &self.text_block) {
                parts.push(StreamPart::TextDelta {
                    id: id.clone(),
                    delta: String::new(),
                    provider_metadata: metadata,
                });
            }
            return;
        }
        if thought {
            self.end_text(parts);
            if self.reasoning_block.is_none() {
                let id = self.next_block_id();
                self.reasoning_block = Some(id.clone());
                parts.push(StreamPart::ReasoningStart {
                    id,
                    provider_metadata: metadata.clone(),
                });
            }
            if let Some(id) = &self.reasoning_block {
                parts.push(StreamPart::ReasoningDelta {
                    id: id.clone(),
                    delta: text.to_owned(),
                    provider_metadata: metadata,
                });
            }
        } else {
            self.end_reasoning(parts);
            if self.text_block.is_none() {
                let id = self.next_block_id();
                self.text_block = Some(id.clone());
                parts.push(StreamPart::TextStart {
                    id,
                    provider_metadata: metadata.clone(),
                });
            }
            if let Some(id) = &self.text_block {
                parts.push(StreamPart::TextDelta {
                    id: id.clone(),
                    delta: text.to_owned(),
                    provider_metadata: metadata,
                });
            }
        }
    }

    fn content_parts(&mut self, part: &Part, parts: &mut Vec<StreamPart>) {
        let signature = part.thought_signature.as_deref();
        if let Some(code) = &part.executable_code
            && code.code.is_some()
        {
            parts.push(StreamPart::ToolCall(self.mapper.code_execution_call(
                code.language.as_deref(),
                code.code.as_deref(),
            )));
        } else if let Some(result) = &part.code_execution_result {
            parts.push(StreamPart::ToolResult(self.mapper.code_execution_result(
                result.outcome.as_deref(),
                result.output.as_deref(),
            )));
        } else if let Some(text) = &part.text {
            self.text_part(text, part.thought == Some(true), signature, parts);
        } else if let Some(inline) = &part.inline_data {
            self.end_text(parts);
            self.end_reasoning(parts);
            match self.mapper.inline_file(
                &inline.mime_type,
                &inline.data,
                part.thought == Some(true),
                signature,
            ) {
                Ok(ferrin_spec::Content::ReasoningFile {
                    data,
                    media_type,
                    provider_metadata,
                }) => parts.push(StreamPart::ReasoningFile {
                    data,
                    media_type,
                    provider_metadata,
                }),
                Ok(ferrin_spec::Content::File {
                    data,
                    media_type,
                    filename,
                    provider_metadata,
                }) => parts.push(StreamPart::File {
                    data,
                    media_type,
                    filename,
                    provider_metadata,
                }),
                Ok(_) => {}
                Err(error) => parts.push(StreamPart::error(&error)),
            }
        } else if let Some(call) = &part.tool_call {
            parts.push(StreamPart::ToolCall(self.mapper.server_tool_call(
                call.tool_type.as_deref(),
                call.args.as_ref(),
                call.id.as_deref(),
                signature,
            )));
        } else if let Some(response) = &part.tool_response {
            parts.push(StreamPart::ToolResult(
                self.mapper.server_tool_response(response),
            ));
        }
    }

    fn complete_call(
        &mut self,
        call: &FunctionCall,
        name: &str,
        metadata: Option<ProviderMetadata>,
        parts: &mut Vec<StreamPart>,
    ) {
        let mapped = self
            .mapper
            .function_call(call.id.as_deref(), name, call.args.as_ref(), None);
        let id = mapped.tool_call_id.clone();
        let tool_name = mapped.tool_name.clone();
        parts.push(StreamPart::ToolInputStart {
            id: id.clone(),
            tool_name: tool_name.clone(),
            provider_executed: false,
            dynamic: false,
            title: None,
            provider_metadata: metadata.clone(),
        });
        if call.args.is_some() {
            parts.push(StreamPart::ToolInputDelta {
                id: id.clone(),
                delta: mapped.input.clone(),
                provider_metadata: metadata.clone(),
            });
        }
        parts.push(StreamPart::ToolInputEnd {
            id: id.clone(),
            provider_metadata: metadata.clone(),
        });
        let mut tool_call = ToolCall::new(id, tool_name, mapped.input);
        tool_call.provider_metadata = metadata;
        parts.push(StreamPart::ToolCall(tool_call));
        self.has_tool_calls = true;
    }

    fn function_call_part(&mut self, part: &Part, parts: &mut Vec<StreamPart>) {
        let Some(call) = &part.function_call else {
            return;
        };
        let metadata = self
            .mapper
            .signature_metadata(part.thought_signature.as_deref());
        if call.is_streaming_fragment() {
            if let Some(name) = &call.name {
                let id = call
                    .id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| self.mapper.generate_id());
                let id = ToolCallId::new(id);
                let tool_name = self.mapper.custom_tool_name(name);
                self.active_calls.push(ActiveToolCall {
                    id: id.clone(),
                    tool_name: tool_name.clone(),
                    accumulator: JsonAccumulator::new(),
                    provider_metadata: metadata.clone(),
                });
                parts.push(StreamPart::ToolInputStart {
                    id,
                    tool_name: tool_name.into(),
                    provider_executed: false,
                    dynamic: false,
                    title: None,
                    provider_metadata: metadata.clone(),
                });
            }
            if let Some(partial_args) = &call.partial_args {
                if let Some(active) = self.active_calls.last_mut() {
                    let delta = active.accumulator.process(partial_args);
                    if !delta.is_empty() {
                        parts.push(StreamPart::ToolInputDelta {
                            id: active.id.clone(),
                            delta,
                            provider_metadata: metadata,
                        });
                    }
                }
                if call.completes_stream() {
                    self.finish_active_call(parts);
                }
            }
        } else if call.is_terminal() {
            if !self.active_calls.is_empty() {
                self.finish_active_call(parts);
            }
        } else if let Some(name) = &call.name {
            // A complete call (`name` + `args`) or a single-chunk call without
            // arguments (`{name}`).
            self.complete_call(call, name, metadata, parts);
        }
    }

    fn handle_chunk(
        &mut self,
        value: &GenerateContentResponse,
        raw: &JsonValue,
    ) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        if !self.emitted_response_metadata
            && let Some(id) = &value.response_id
        {
            self.emitted_response_metadata = true;
            parts.push(StreamPart::ResponseMetadata {
                id: Some(id.clone()),
                timestamp: None,
                model_id: None,
            });
        }
        if let Some(usage) = &value.usage_metadata {
            self.usage = Some(usage.clone());
            self.raw_usage = raw
                .get("usageMetadata")
                .and_then(JsonValue::as_object)
                .cloned();
        }
        let Some(candidate) = value.candidate() else {
            if let Some(reason) = value.block_reason() {
                self.received_finish_reason = true;
                self.finish_reason =
                    FinishReason::with_raw(FinishReasonKind::ContentFilter, reason);
                self.provider_metadata = Some(self.finish_metadata(value, None, None));
            }
            return parts;
        };
        if candidate.grounding_metadata.is_some() {
            self.last_grounding_metadata = candidate.grounding_metadata.clone();
        }
        if candidate.url_context_metadata.is_some() {
            self.last_url_context_metadata = candidate.url_context_metadata.clone();
        }
        for source in self.mapper.sources(&candidate.grounding_chunks()) {
            if let Source::Url { url, .. } = &source {
                if self.emitted_source_urls.insert(url.clone()) {
                    parts.push(StreamPart::Source(source));
                }
            } else {
                parts.push(StreamPart::Source(source));
            }
        }
        let candidate_parts = candidate.parts().to_vec();
        for part in &candidate_parts {
            self.content_parts(part, &mut parts);
        }
        for part in &candidate_parts {
            self.function_call_part(part, &mut parts);
        }
        let block_reason = value.block_reason();
        let prompt_blocked = candidate.finish_reason.is_none() && block_reason.is_some();
        if let Some(raw_reason) = candidate.finish_reason.as_deref().or(block_reason) {
            self.received_finish_reason = true;
            self.finish_reason = if prompt_blocked {
                FinishReason::with_raw(FinishReasonKind::ContentFilter, raw_reason)
            } else {
                map_finish_reason(Some(raw_reason), self.has_tool_calls)
            };
            self.provider_metadata = Some(self.finish_metadata(
                value,
                candidate.safety_ratings.as_ref(),
                candidate.finish_message.as_deref(),
            ));
        }
        parts
    }
}

impl StreamMachine for GoogleStreamState {
    type Chunk = GenerateContentResponse;

    fn handle(
        &mut self,
        chunk: ParseResult<GenerateContentResponse>,
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
                parts.extend(self.handle_chunk(&value, &raw));
            }
            ParseResult::Err { error, raw } => {
                if include_raw {
                    parts.push(StreamPart::Raw {
                        raw_value: raw.map_or(JsonValue::Null, JsonValue::from),
                    });
                }
                parts.push(StreamPart::error(&error));
            }
        }
        parts
    }

    fn finish(mut self) -> Vec<StreamPart> {
        if !self.received_finish_reason || !self.active_calls.is_empty() {
            return vec![StreamPart::error(&ProviderError::from(
                InvalidResponseDataError::new(
                    "google stream ended before completion",
                    JsonValue::Null,
                ),
            ))];
        }
        let mut parts = Vec::new();
        self.end_text(&mut parts);
        self.end_reasoning(&mut parts);
        parts.push(StreamPart::Finish {
            finish_reason: self.finish_reason,
            usage: convert_usage(self.usage.as_ref(), self.raw_usage),
            provider_metadata: self.provider_metadata,
        });
        parts
    }
}
