//! Stream state machine of the chat model.

use std::collections::BTreeMap;
use std::collections::HashMap;

use ferrin_provider_util::ParseResult;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::StreamPart;

use super::api_types::ChatResponse;
use super::api_types::ChatToolCall;
use super::api_types::ChatUsage;
use super::output::convert_content;
use super::output::convert_usage;
use super::output::map_finish_reason;
use super::output::prediction_metadata;
use crate::config::SharedConfig;
use crate::error::stream_error_for_frame;
use crate::metadata::StreamMetadataExtractor;
use crate::metadata::merge_metadata;
use crate::metadata::metadata_under;
use crate::metadata::timestamp_from_seconds;

/// Id of the single text part.
const TEXT_PART_ID: &str = "txt-0";
/// Id of the single reasoning part.
const REASONING_PART_ID: &str = "reasoning-0";

#[derive(Debug)]
struct TrackedToolCall {
    id: String,
    name: String,
    arguments: String,
    metadata: Option<ProviderMetadata>,
}

/// A tool call delta buffered until its function name arrives (some
/// endpoints send the first delta without it).
#[derive(Debug, Default)]
struct PendingToolCall {
    id: Option<String>,
    arguments: String,
    extra_content: Option<JsonValue>,
}

/// Tracks streamed tool calls by id and index.
#[derive(Debug, Default)]
struct ToolCallTracker {
    calls: Vec<TrackedToolCall>,
    by_id: HashMap<String, usize>,
    by_index: HashMap<usize, usize>,
    latest: Option<usize>,
    pending: BTreeMap<usize, PendingToolCall>,
    forwarded: Vec<usize>,
}

impl ToolCallTracker {
    /// Buffers deltas without a name by index, forwarding the accumulated
    /// delta once the name is known; deltas without an index go straight
    /// to the tracker.
    fn process(
        &mut self,
        delta: &ChatToolCall,
        metadata_key: &str,
        config: &SharedConfig,
        parts: &mut Vec<StreamPart>,
    ) {
        let Some(index) = delta.index else {
            self.process_delta(delta, metadata_key, config, parts);
            return;
        };
        if self.forwarded.contains(&index) {
            self.process_delta(delta, metadata_key, config, parts);
            return;
        }
        let pending = self.pending.entry(index).or_default();
        if pending.id.is_none() && delta.id.is_some() {
            pending.id = delta.id.clone();
        }
        if pending.extra_content.is_none() && delta.extra_content.is_some() {
            pending.extra_content = delta.extra_content.clone();
        }
        if let Some(arguments) = delta.function.as_ref().and_then(|f| f.arguments.as_deref()) {
            pending.arguments.push_str(arguments);
        }
        let Some(name) = delta.function.as_ref().and_then(|f| f.name.clone()) else {
            return;
        };
        let Some(pending) = self.pending.remove(&index) else {
            return;
        };
        let forward = ChatToolCall {
            index: Some(index),
            id: pending.id,
            function: Some(super::api_types::ChatFunction {
                name: Some(name),
                arguments: Some(pending.arguments),
            }),
            extra_content: pending.extra_content,
        };
        self.forwarded.push(index);
        self.process_delta(&forward, metadata_key, config, parts);
    }

    fn process_delta(
        &mut self,
        delta: &ChatToolCall,
        metadata_key: &str,
        config: &SharedConfig,
        parts: &mut Vec<StreamPart>,
    ) {
        let existing = match (&delta.id, delta.index) {
            (Some(id), _) if !id.is_empty() && self.by_id.contains_key(id) => {
                self.by_id.get(id).copied()
            }
            (_, Some(index)) => self.by_index.get(&index).copied(),
            _ => self.latest,
        };
        let position = match existing {
            Some(position) => {
                self.process_existing(position, delta, parts);
                position
            }
            None => match self.process_new(delta, metadata_key, config, parts) {
                Ok(position) => position,
                Err(error) => {
                    parts.push(StreamPart::error(&error));
                    return;
                }
            },
        };
        if let Some(index) = delta.index {
            self.by_index.insert(index, position);
        }
        self.latest = Some(position);
    }

    fn process_new(
        &mut self,
        delta: &ChatToolCall,
        metadata_key: &str,
        config: &SharedConfig,
        parts: &mut Vec<StreamPart>,
    ) -> Result<usize, ProviderError> {
        let Some(name) = delta.function.as_ref().and_then(|f| f.name.clone()) else {
            return Err(InvalidResponseDataError::new(
                "expected tool call delta to carry a function name",
                JsonValue::Null,
            )
            .into());
        };
        let id = delta
            .id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| config.generate_id());
        let arguments = delta
            .function
            .as_ref()
            .and_then(|f| f.arguments.clone())
            .unwrap_or_default();
        let metadata = delta.thought_signature().map(|signature| {
            let mut object = JsonObject::new();
            object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
            metadata_under(metadata_key, object)
        });
        parts.push(StreamPart::ToolInputStart {
            id: ToolCallId::new(id.clone()),
            tool_name: name.clone().into(),
            provider_executed: false,
            dynamic: false,
            title: None,
            provider_metadata: metadata.clone(),
        });
        if !arguments.is_empty() {
            parts.push(StreamPart::ToolInputDelta {
                id: ToolCallId::new(id.clone()),
                delta: arguments.clone(),
                provider_metadata: None,
            });
        }
        let position = self.calls.len();
        self.by_id.insert(id.clone(), position);
        self.calls.push(TrackedToolCall {
            id,
            name,
            arguments,
            metadata,
        });
        Ok(position)
    }

    fn process_existing(
        &mut self,
        position: usize,
        delta: &ChatToolCall,
        parts: &mut Vec<StreamPart>,
    ) {
        let Some(call) = self.calls.get_mut(position) else {
            return;
        };
        if call.metadata.is_none()
            && let Some(signature) = delta.thought_signature()
        {
            let mut object = JsonObject::new();
            object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
            call.metadata = Some(metadata_under("", object));
        }
        if let Some(arguments) = delta.function.as_ref().and_then(|f| f.arguments.as_deref())
            && !arguments.is_empty()
        {
            call.arguments.push_str(arguments);
            parts.push(StreamPart::ToolInputDelta {
                id: ToolCallId::new(call.id.clone()),
                delta: arguments.to_owned(),
                provider_metadata: None,
            });
        }
    }

    /// Emits errors for calls that never received a name, then the closing
    /// parts of every tracked call.
    fn flush(self, metadata_key: &str, parts: &mut Vec<StreamPart>) {
        if !self.pending.is_empty() {
            let error: ProviderError = InvalidResponseDataError::new(
                "expected tool call delta to carry a function name",
                JsonValue::Null,
            )
            .into();
            parts.push(StreamPart::error(&error));
            return;
        }
        for call in self.calls {
            parts.push(StreamPart::ToolInputEnd {
                id: ToolCallId::new(call.id.clone()),
                provider_metadata: None,
            });
            let mut tool_call = ToolCall::new(call.id, call.name, call.arguments);
            tool_call.provider_metadata = call.metadata.map(|metadata| {
                // Metadata captured on a later delta was stored under an
                // empty key; move it under the provider key.
                let mut fixed = ProviderMetadata::new();
                for (key, value) in metadata {
                    let key = if key.is_empty() {
                        metadata_key.to_owned()
                    } else {
                        key
                    };
                    fixed.insert(key, value);
                }
                fixed
            });
            parts.push(StreamPart::ToolCall(tool_call));
        }
    }
}

/// State of a streamed chat completion.
pub struct ChatStreamState {
    config: SharedConfig,
    metadata_key: String,
    extractor: Option<Box<dyn StreamMetadataExtractor>>,
    finish_reason: Option<FinishReason>,
    usage: Option<(ChatUsage, Option<JsonObject>)>,
    first_chunk: bool,
    text_active: bool,
    reasoning_active: bool,
    tool_calls: ToolCallTracker,
}

impl std::fmt::Debug for ChatStreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatStreamState")
            .field("metadata_key", &self.metadata_key)
            .field("finish_reason", &self.finish_reason)
            .field("text_active", &self.text_active)
            .field("reasoning_active", &self.reasoning_active)
            .finish_non_exhaustive()
    }
}

impl ChatStreamState {
    /// Creates the state.
    #[must_use]
    pub fn new(
        config: SharedConfig,
        metadata_key: String,
        extractor: Option<Box<dyn StreamMetadataExtractor>>,
    ) -> Self {
        Self {
            config,
            metadata_key,
            extractor,
            finish_reason: None,
            usage: None,
            first_chunk: true,
            text_active: false,
            reasoning_active: false,
            tool_calls: ToolCallTracker::default(),
        }
    }

    fn end_text(&mut self, parts: &mut Vec<StreamPart>) {
        if self.text_active {
            self.text_active = false;
            parts.push(StreamPart::TextEnd {
                id: PartId::new(TEXT_PART_ID),
                provider_metadata: None,
            });
        }
    }

    fn end_reasoning(&mut self, parts: &mut Vec<StreamPart>) {
        if self.reasoning_active {
            self.reasoning_active = false;
            parts.push(StreamPart::ReasoningEnd {
                id: PartId::new(REASONING_PART_ID),
                provider_metadata: None,
            });
        }
    }

    fn reasoning_delta(&mut self, delta: &str, parts: &mut Vec<StreamPart>) {
        self.end_text(parts);
        if !self.reasoning_active {
            self.reasoning_active = true;
            parts.push(StreamPart::ReasoningStart {
                id: PartId::new(REASONING_PART_ID),
                provider_metadata: None,
            });
        }
        parts.push(StreamPart::ReasoningDelta {
            id: PartId::new(REASONING_PART_ID),
            delta: delta.to_owned(),
            provider_metadata: None,
        });
    }

    fn text_delta(&mut self, delta: &str, parts: &mut Vec<StreamPart>) {
        self.end_reasoning(parts);
        if !self.text_active {
            self.text_active = true;
            parts.push(StreamPart::TextStart {
                id: PartId::new(TEXT_PART_ID),
                provider_metadata: None,
            });
        }
        parts.push(StreamPart::text_delta(PartId::new(TEXT_PART_ID), delta));
    }

    fn handle_value(&mut self, value: &ChatResponse, raw: &JsonValue, parts: &mut Vec<StreamPart>) {
        if let Some(extractor) = &mut self.extractor {
            extractor.process_chunk(raw);
        }
        if value.error.is_some() {
            self.finish_reason = Some(FinishReason::error());
            parts.push(StreamPart::Error {
                error: stream_error_for_frame(self.config.error_structure.as_ref(), raw),
            });
            return;
        }
        if self.first_chunk {
            self.first_chunk = false;
            parts.push(StreamPart::ResponseMetadata {
                id: value.id.clone(),
                timestamp: timestamp_from_seconds(value.created),
                model_id: value.model.clone().map(Into::into),
            });
        }
        if let Some(usage) = &value.usage {
            let raw_usage = raw.get("usage").and_then(JsonValue::as_object).cloned();
            self.usage = Some((usage.clone(), raw_usage));
        }
        let Some(choice) = value.choices.as_ref().and_then(|c| c.first()) else {
            return;
        };
        if let Some(reason) = &choice.finish_reason {
            self.finish_reason = Some(map_finish_reason(reason));
        }
        let Some(delta) = &choice.delta else {
            return;
        };
        if let Some(reasoning) = delta.reasoning_text().filter(|text| !text.is_empty()) {
            self.reasoning_delta(reasoning, parts);
        }
        for content in convert_content(delta.content.as_ref()) {
            match content {
                Content::Reasoning { text, .. } => self.reasoning_delta(&text, parts),
                Content::Text { text, .. } => self.text_delta(&text, parts),
                _ => {}
            }
        }
        if let Some(tool_calls) = &delta.tool_calls
            && !tool_calls.is_empty()
        {
            self.end_reasoning(parts);
            for call in tool_calls {
                self.tool_calls
                    .process(call, &self.metadata_key, &self.config, parts);
            }
        }
    }
}

impl StreamMachine for ChatStreamState {
    type Chunk = ChatResponse;

    fn handle(&mut self, chunk: ParseResult<ChatResponse>, include_raw: bool) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        match chunk {
            ParseResult::Ok { value, raw } => {
                if include_raw {
                    parts.push(StreamPart::Raw {
                        raw_value: raw.clone(),
                    });
                }
                self.handle_value(&value, &raw, &mut parts);
            }
            ParseResult::Err { error, raw } => {
                if include_raw && let Some(raw) = raw {
                    parts.push(StreamPart::Raw {
                        raw_value: JsonValue::from(raw),
                    });
                }
                self.finish_reason = Some(FinishReason::error());
                parts.push(StreamPart::error(&error));
            }
        }
        parts
    }

    fn finish(mut self) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        let Some(finish_reason) = self.finish_reason.take() else {
            let error: ProviderError = InvalidResponseDataError::new(
                "response stream ended without a finish reason",
                JsonValue::Null,
            )
            .into();
            parts.push(StreamPart::error(&error));
            return parts;
        };
        self.end_reasoning(&mut parts);
        self.end_text(&mut parts);
        let tracker = std::mem::take(&mut self.tool_calls);
        tracker.flush(&self.metadata_key, &mut parts);
        let mut provider_metadata = metadata_under(
            &self.metadata_key,
            self.usage
                .as_ref()
                .map(|(usage, _)| prediction_metadata(usage))
                .unwrap_or_default(),
        );
        if let Some(extractor) = &mut self.extractor
            && let Some(extra) = extractor.build_metadata()
        {
            merge_metadata(&mut provider_metadata, extra);
        }
        let usage = match &self.usage {
            Some((usage, raw)) => match &self.config.convert_usage {
                Some(convert) => convert(usage),
                None => convert_usage(usage, raw.clone()),
            },
            None => Usage::default(),
        };
        parts.push(StreamPart::Finish {
            finish_reason,
            usage,
            provider_metadata: Some(provider_metadata),
        });
        parts
    }
}
