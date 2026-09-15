//! Stream state machine of the Chat Completions API.

use std::collections::HashMap;

use ferrin_provider_util::ParseResult;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::StreamPart;

use super::api_types::ChatResponse;
use super::api_types::ChatToolCall;
use super::api_types::ChatUsage;
use super::output::map_chat_finish_reason;
use super::output::map_chat_usage;
use super::output::prediction_metadata;
use crate::config::SharedConfig;
use crate::error::stream_error_for_frame;
use crate::responses::output::metadata;
use crate::stream_util::StreamMachine;
use crate::stream_util::timestamp_from_seconds;

/// Id of the single text part of a completion.
const TEXT_PART_ID: &str = "0";

#[derive(Debug)]
struct TrackedToolCall {
    id: String,
    name: String,
    arguments: String,
    finished: bool,
}

/// Tracks streamed tool calls by index and id.
#[derive(Debug, Default)]
struct ToolCallTracker {
    calls: Vec<TrackedToolCall>,
    by_id: HashMap<String, usize>,
    by_index: HashMap<usize, usize>,
    latest: Option<usize>,
}

impl ToolCallTracker {
    fn process_delta(&mut self, delta: &ChatToolCall, parts: &mut Vec<StreamPart>) {
        let existing = match (&delta.id, delta.index) {
            (Some(id), _) if !id.is_empty() => self.by_id.get(id).copied(),
            (_, Some(index)) => self.by_index.get(&index).copied(),
            _ => self.latest,
        };
        let position = match existing {
            Some(position) => {
                self.process_existing(position, delta, parts);
                position
            }
            None => match self.process_new(delta, parts) {
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
        parts: &mut Vec<StreamPart>,
    ) -> Result<usize, ProviderError> {
        let Some(id) = delta.id.clone() else {
            return Err(InvalidResponseDataError::new(
                "expected tool call delta to carry an id",
                JsonValue::Null,
            )
            .into());
        };
        let Some(name) = delta.function.as_ref().and_then(|f| f.name.clone()) else {
            return Err(InvalidResponseDataError::new(
                "expected tool call delta to carry a function name",
                JsonValue::Null,
            )
            .into());
        };
        let arguments = delta
            .function
            .as_ref()
            .and_then(|f| f.arguments.clone())
            .unwrap_or_default();
        parts.push(StreamPart::ToolInputStart {
            id: ToolCallId::new(id.clone()),
            tool_name: name.clone().into(),
            provider_executed: false,
            dynamic: false,
            title: None,
            provider_metadata: None,
        });
        if !arguments.is_empty() {
            parts.push(StreamPart::ToolInputDelta {
                id: ToolCallId::new(id.clone()),
                delta: arguments.clone(),
                provider_metadata: None,
            });
        }
        let position = self.calls.len();
        if !id.is_empty() {
            self.by_id.insert(id.clone(), position);
        }
        self.calls.push(TrackedToolCall {
            id,
            name,
            arguments,
            finished: false,
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
        if call.finished {
            return;
        }
        if let Some(arguments) = delta.function.as_ref().and_then(|f| f.arguments.as_deref()) {
            call.arguments.push_str(arguments);
            parts.push(StreamPart::ToolInputDelta {
                id: ToolCallId::new(call.id.clone()),
                delta: arguments.to_owned(),
                provider_metadata: None,
            });
        }
    }

    fn flush(&mut self, parts: &mut Vec<StreamPart>) {
        for call in &mut self.calls {
            if call.finished {
                continue;
            }
            call.finished = true;
            parts.push(StreamPart::ToolInputEnd {
                id: ToolCallId::new(call.id.clone()),
                provider_metadata: None,
            });
            parts.push(StreamPart::ToolCall(ToolCall::new(
                call.id.clone(),
                call.name.clone(),
                call.arguments.clone(),
            )));
        }
    }
}

/// State of a streamed completion.
#[derive(Debug)]
pub struct ChatStreamState {
    config: SharedConfig,
    finish_reason: FinishReason,
    received_finish_reason: bool,
    usage: Option<ChatUsage>,
    raw_usage: Option<JsonObject>,
    metadata_extracted: bool,
    text_active: bool,
    provider_metadata: JsonObject,
    tool_calls: ToolCallTracker,
}

impl ChatStreamState {
    /// Creates the state.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            config,
            finish_reason: FinishReason::new(ferrin_spec::FinishReasonKind::Other),
            received_finish_reason: false,
            usage: None,
            raw_usage: None,
            metadata_extracted: false,
            text_active: false,
            provider_metadata: JsonObject::new(),
            tool_calls: ToolCallTracker::default(),
        }
    }

    fn handle_value(&mut self, value: &ChatResponse, raw: &JsonValue, parts: &mut Vec<StreamPart>) {
        if value.error.is_some() {
            self.finish_reason = FinishReason::error();
            parts.push(StreamPart::Error {
                error: stream_error_for_frame(raw),
            });
            return;
        }
        if !self.metadata_extracted {
            self.metadata_extracted = true;
            parts.push(StreamPart::ResponseMetadata {
                id: value.id.clone(),
                timestamp: timestamp_from_seconds(value.created),
                model_id: value.model.clone().map(Into::into),
            });
        }
        if let Some(usage) = &value.usage {
            self.usage = Some(usage.clone());
            self.raw_usage = raw.get("usage").and_then(JsonValue::as_object).cloned();
            for (key, count) in prediction_metadata(usage) {
                self.provider_metadata.insert(key, JsonValue::from(count));
            }
        }
        let Some(choice) = value.choices.as_ref().and_then(|c| c.first()) else {
            return;
        };
        if let Some(reason) = &choice.finish_reason {
            self.received_finish_reason = true;
            self.finish_reason = map_chat_finish_reason(reason);
        }
        if let Some(content) = choice.logprobs.as_ref().and_then(|l| l.content.clone()) {
            self.provider_metadata
                .insert("logprobs".to_owned(), JsonValue::Array(content));
        }
        let Some(delta) = &choice.delta else {
            return;
        };
        if let Some(content) = &delta.content {
            if !self.text_active {
                self.text_active = true;
                parts.push(StreamPart::TextStart {
                    id: PartId::new(TEXT_PART_ID),
                    provider_metadata: None,
                });
            }
            parts.push(StreamPart::TextDelta {
                id: PartId::new(TEXT_PART_ID),
                delta: content.clone(),
                provider_metadata: None,
            });
        }
        if let Some(tool_calls) = &delta.tool_calls {
            for call in tool_calls {
                self.tool_calls.process_delta(call, parts);
            }
        }
        if let Some(annotations) = &delta.annotations {
            for annotation in annotations {
                if let Some(citation) = &annotation.url_citation
                    && annotation.kind == "url_citation"
                {
                    parts.push(StreamPart::Source(Source::Url {
                        id: self.config.generate_id(),
                        url: citation.url.clone(),
                        title: citation.title.clone(),
                        provider_metadata: None,
                    }));
                }
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
                self.finish_reason = FinishReason::error();
                parts.push(StreamPart::error(&error));
            }
        }
        parts
    }

    fn finish(mut self) -> Vec<StreamPart> {
        if !self.received_finish_reason {
            return vec![StreamPart::error(&ProviderError::from(
                InvalidResponseDataError::new(
                    "chat stream ended before a finish reason was received",
                    JsonValue::Null,
                ),
            ))];
        }
        let mut parts = Vec::new();
        if self.text_active {
            parts.push(StreamPart::TextEnd {
                id: PartId::new(TEXT_PART_ID),
                provider_metadata: None,
            });
        }
        self.tool_calls.flush(&mut parts);
        let usage = self.usage.as_ref().map_or_else(Usage::default, |usage| {
            map_chat_usage(usage, self.raw_usage.clone())
        });
        parts.push(StreamPart::Finish {
            finish_reason: self.finish_reason,
            usage,
            provider_metadata: Some(metadata(
                &self.config.provider_options_key,
                self.provider_metadata,
            )),
        });
        parts
    }
}
