//! Streaming state machine of the Messages API.

use std::collections::HashMap;

use ferrin_provider_util::ParseResult;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::FinishReason;
use ferrin_spec::language_model::FinishReasonKind;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::ToolCall;
use serde_json::json;

use crate::api_types::AnthropicChunk;
use crate::api_types::AnthropicUsage;
use crate::api_types::ContentBlock;
use crate::api_types::Delta;
use crate::api_types::MessageStart;
use crate::api_types::StopDetails;
use crate::error::FrameError;
use crate::output::MessageMetadata;
use crate::output::OutputMapper;
use crate::output::anthropic_metadata;
use crate::output::caller_metadata;
use crate::output::container_metadata;
use crate::output::programmatic_tool_call;
use crate::usage::convert_usage;
use crate::usage::map_stop_reason;

#[derive(Debug)]
struct ToolCallBlock {
    tool_call_id: String,
    tool_name: ToolName,
    input: String,
    provider_executed: bool,
    dynamic: bool,
    first_delta: bool,
    provider_tool_name: Option<String>,
    provider_tool_input_type: Option<&'static str>,
    caller: Option<ProviderMetadata>,
}

#[derive(Debug)]
enum Block {
    Text { citations: Vec<JsonValue> },
    Reasoning { thinking: bool },
    ToolCall(Box<ToolCallBlock>),
}

/// State of one streamed message.
#[derive(Debug)]
pub struct AnthropicStreamState {
    mapper: OutputMapper,
    custom_key: Option<String>,
    finish_reason: FinishReason,
    usage: AnthropicUsage,
    raw_usage: Option<JsonObject>,
    blocks: HashMap<u64, Block>,
    stop_sequence: Option<String>,
    stop_details: Option<StopDetails>,
    input_transformations: Option<JsonValue>,
    container: Option<JsonValue>,
    context_management: Option<crate::api_types::ResponseContextManagement>,
    message_open: bool,
    active_message_id: Option<String>,
    invalid_sequence: bool,
}

fn part_id(index: u64) -> PartId {
    PartId::new(index.to_string())
}

fn content_to_parts(content: Vec<Content>) -> Vec<StreamPart> {
    content
        .into_iter()
        .filter_map(|part| match part {
            Content::ToolCall(call) => Some(StreamPart::ToolCall(call)),
            Content::ToolResult(result) => Some(StreamPart::ToolResult(result)),
            Content::Source(source) => Some(StreamPart::Source(source)),
            _ => None,
        })
        .collect()
}

fn tool_input_start(block: &ToolCallBlock) -> StreamPart {
    StreamPart::ToolInputStart {
        id: ToolCallId::new(block.tool_call_id.clone()),
        tool_name: block.tool_name.clone(),
        provider_executed: block.provider_executed,
        dynamic: block.dynamic,
        title: None,
        provider_metadata: None,
    }
}

impl AnthropicStreamState {
    /// Creates the state; `custom_key` duplicates the response metadata
    /// under the configured provider name.
    #[must_use]
    pub fn new(mapper: OutputMapper, custom_key: Option<String>) -> Self {
        Self {
            mapper,
            custom_key,
            finish_reason: FinishReason::new(FinishReasonKind::Other),
            usage: AnthropicUsage::default(),
            raw_usage: None,
            blocks: HashMap::new(),
            stop_sequence: None,
            stop_details: None,
            input_transformations: None,
            container: None,
            context_management: None,
            message_open: false,
            active_message_id: None,
            invalid_sequence: false,
        }
    }

    fn handle_chunk(&mut self, chunk: AnthropicChunk, raw: &JsonValue) -> Vec<StreamPart> {
        match chunk {
            AnthropicChunk::Ping | AnthropicChunk::Unknown => Vec::new(),
            AnthropicChunk::ContentBlockStart {
                index,
                content_block,
            } => self.block_start(index, content_block),
            AnthropicChunk::ContentBlockStop { index } => self.block_stop(index),
            AnthropicChunk::ContentBlockDelta { index, delta } => self.block_delta(index, delta),
            AnthropicChunk::MessageStart { message } => self.message_start(message, raw),
            AnthropicChunk::MessageDelta {
                delta,
                usage,
                input_transformations,
                context_management,
            } => {
                if let Some(input) = usage.input_tokens
                    && self.usage.input_tokens != Some(input)
                {
                    self.usage.input_tokens = Some(input);
                }
                self.usage.output_tokens = usage.output_tokens;
                if usage.output_tokens_details.is_some() {
                    self.usage.output_tokens_details = usage.output_tokens_details;
                }
                if usage.cache_read_input_tokens.is_some() {
                    self.usage.cache_read_input_tokens = usage.cache_read_input_tokens;
                }
                if usage.cache_creation_input_tokens.is_some() {
                    self.usage.cache_creation_input_tokens = usage.cache_creation_input_tokens;
                }
                if usage.iterations.is_some() {
                    self.usage.iterations = usage.iterations;
                }
                self.finish_reason = map_stop_reason(
                    delta.stop_reason.as_deref(),
                    self.mapper.json_response_from_tool,
                );
                self.stop_sequence = delta.stop_sequence;
                self.stop_details = delta.stop_details;
                self.container = delta
                    .container
                    .as_ref()
                    .map(|container| container_metadata(container, true));
                if context_management.is_some() {
                    self.context_management = context_management;
                }
                if input_transformations.is_some() {
                    self.input_transformations = input_transformations;
                }
                let mut merged = self.raw_usage.take().unwrap_or_default();
                if let Some(JsonValue::Object(usage)) = raw.get("usage") {
                    merged.extend(usage.clone());
                }
                self.raw_usage = Some(merged);
                Vec::new()
            }
            AnthropicChunk::MessageStop => {
                self.message_open = false;
                self.active_message_id = None;
                let metadata = MessageMetadata {
                    usage: self.raw_usage.clone(),
                    stop_sequence: self.stop_sequence.clone(),
                    stop_details: self.stop_details.as_ref(),
                    input_transformations: self.input_transformations.as_ref(),
                    iterations: self.usage.iterations.as_deref(),
                    container: self.container.clone(),
                    context_management: self.context_management.as_ref(),
                }
                .build(self.custom_key.as_deref());
                vec![StreamPart::Finish {
                    finish_reason: self.finish_reason.clone(),
                    usage: convert_usage(&self.usage, self.raw_usage.clone()),
                    provider_metadata: Some(metadata),
                }]
            }
            AnthropicChunk::Error { error } => {
                let frame = JsonValue::Object(error);
                vec![StreamPart::Error {
                    error: FrameError::from_error_object(&frame).to_stream_error(&frame),
                }]
            }
        }
    }

    fn message_start(&mut self, message: MessageStart, raw: &JsonValue) -> Vec<StreamPart> {
        if self.message_open {
            if self.active_message_id == message.id {
                return Vec::new();
            }
            self.invalid_sequence = true;
            let error = InvalidResponseDataError::new(
                format!(
                    "received message_start for message {} while message {} is still open",
                    json!(message.id),
                    json!(self.active_message_id)
                ),
                raw.clone(),
            );
            return vec![StreamPart::error(&error.into())];
        }
        self.message_open = true;
        self.active_message_id = message.id.clone();
        self.usage.input_tokens = Some(message.usage.input_tokens.unwrap_or(0));
        self.usage.cache_read_input_tokens =
            Some(message.usage.cache_read_input_tokens.unwrap_or(0));
        self.usage.cache_creation_input_tokens =
            Some(message.usage.cache_creation_input_tokens.unwrap_or(0));
        self.raw_usage = raw
            .get("message")
            .and_then(|message| message.get("usage"))
            .and_then(JsonValue::as_object)
            .cloned();
        if message.input_transformations.is_some() {
            self.input_transformations = message.input_transformations;
        }
        if let Some(container) = &message.container {
            self.container = Some(container_metadata(container, false));
        }
        if message.stop_reason.is_some() {
            self.finish_reason = map_stop_reason(
                message.stop_reason.as_deref(),
                self.mapper.json_response_from_tool,
            );
        }
        let mut parts = vec![StreamPart::ResponseMetadata {
            id: message.id,
            timestamp: None,
            model_id: message.model.map(Into::into),
        }];
        for block in message.content.unwrap_or_default() {
            let ContentBlock::ToolUse {
                id,
                name,
                input,
                caller,
            } = block
            else {
                continue;
            };
            let input = input
                .unwrap_or(JsonValue::Object(JsonObject::new()))
                .to_string();
            parts.push(StreamPart::ToolInputStart {
                id: ToolCallId::new(id.clone()),
                tool_name: self.mapper.custom_name(&name),
                provider_executed: false,
                dynamic: false,
                title: None,
                provider_metadata: None,
            });
            parts.push(StreamPart::ToolInputDelta {
                id: ToolCallId::new(id.clone()),
                delta: input.clone(),
                provider_metadata: None,
            });
            parts.push(StreamPart::ToolInputEnd {
                id: ToolCallId::new(id.clone()),
                provider_metadata: None,
            });
            let mut call = ToolCall::new(id, self.mapper.custom_name(&name), input);
            call.provider_metadata = caller_metadata(caller.as_ref());
            parts.push(StreamPart::ToolCall(call));
        }
        parts
    }

    #[allow(clippy::too_many_lines, reason = "one arm per content block type")]
    fn block_start(&mut self, index: u64, block: ContentBlock) -> Vec<StreamPart> {
        match block {
            ContentBlock::Text { .. } => {
                if self.mapper.uses_json_response_tool {
                    return Vec::new();
                }
                self.blocks.insert(
                    index,
                    Block::Text {
                        citations: Vec::new(),
                    },
                );
                vec![StreamPart::TextStart {
                    id: part_id(index),
                    provider_metadata: None,
                }]
            }
            ContentBlock::Thinking { .. } => {
                self.blocks
                    .insert(index, Block::Reasoning { thinking: true });
                vec![StreamPart::ReasoningStart {
                    id: part_id(index),
                    provider_metadata: None,
                }]
            }
            ContentBlock::RedactedThinking { data } => {
                self.blocks
                    .insert(index, Block::Reasoning { thinking: false });
                let mut meta = JsonObject::new();
                meta.insert("redactedData".to_owned(), JsonValue::from(data));
                vec![StreamPart::ReasoningStart {
                    id: part_id(index),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::Compaction { .. } => {
                self.blocks.insert(
                    index,
                    Block::Text {
                        citations: Vec::new(),
                    },
                );
                let mut meta = JsonObject::new();
                meta.insert("type".to_owned(), JsonValue::from("compaction"));
                vec![StreamPart::TextStart {
                    id: part_id(index),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::ToolUse {
                id,
                name,
                input,
                caller,
            } => {
                if self.mapper.uses_json_response_tool && name == "json" {
                    self.mapper.json_response_from_tool = true;
                    self.blocks.insert(
                        index,
                        Block::Text {
                            citations: Vec::new(),
                        },
                    );
                    return vec![StreamPart::TextStart {
                        id: part_id(index),
                        provider_metadata: None,
                    }];
                }
                let initial = match input {
                    Some(JsonValue::Object(object)) if !object.is_empty() => {
                        JsonValue::Object(object).to_string()
                    }
                    _ => String::new(),
                };
                let block = ToolCallBlock {
                    tool_call_id: id,
                    tool_name: self.mapper.custom_name(&name),
                    first_delta: initial.is_empty(),
                    input: initial,
                    provider_executed: false,
                    dynamic: false,
                    provider_tool_name: None,
                    provider_tool_input_type: None,
                    caller: caller_metadata(caller.as_ref()),
                };
                let start = tool_input_start(&block);
                self.blocks.insert(index, Block::ToolCall(Box::new(block)));
                vec![start]
            }
            ContentBlock::ServerToolUse {
                id,
                name,
                input,
                caller,
            } => {
                let (provider_name, input_type, initial) = match name.as_str() {
                    "text_editor_code_execution" => (
                        "code_execution",
                        Some("text_editor_code_execution"),
                        non_empty_input(input.as_ref()),
                    ),
                    "bash_code_execution" => (
                        "code_execution",
                        Some("bash_code_execution"),
                        non_empty_input(input.as_ref()),
                    ),
                    "code_execution" => (
                        "code_execution",
                        Some("programmatic-tool-call"),
                        non_empty_input(input.as_ref()),
                    ),
                    "web_fetch" | "web_search" => {
                        (name.as_str(), None, non_empty_input(input.as_ref()))
                    }
                    "tool_search_tool_regex" | "tool_search_tool_bm25" => {
                        self.mapper.remember_tool_search(&id, &name);
                        (name.as_str(), None, String::new())
                    }
                    "advisor" => ("advisor", None, "{}".to_owned()),
                    _ => return Vec::new(),
                };
                let block = ToolCallBlock {
                    tool_call_id: id,
                    tool_name: self.mapper.custom_name(provider_name),
                    first_delta: initial.is_empty() || provider_name == "advisor",
                    input: initial,
                    provider_executed: true,
                    dynamic: self.mapper.is_dynamic(provider_name),
                    provider_tool_name: Some(provider_name.to_owned()),
                    provider_tool_input_type: input_type,
                    caller: caller_metadata(caller.as_ref()),
                };
                let start = tool_input_start(&block);
                self.blocks.insert(index, Block::ToolCall(Box::new(block)));
                vec![start]
            }
            ContentBlock::McpToolUse {
                id,
                name,
                input,
                server_name,
            } => vec![StreamPart::ToolCall(self.mapper.mcp_tool_use(
                &id,
                &name,
                input.as_ref(),
                &server_name,
            ))],
            ContentBlock::Fallback { .. } | ContentBlock::Unknown => Vec::new(),
            other => content_to_parts(self.mapper.map_result_block(&other).unwrap_or_default()),
        }
    }

    fn block_stop(&mut self, index: u64) -> Vec<StreamPart> {
        let Some(block) = self.blocks.remove(&index) else {
            return Vec::new();
        };
        match block {
            Block::Text { citations } => {
                let provider_metadata = (!citations.is_empty()).then(|| {
                    let mut meta = JsonObject::new();
                    meta.insert("citations".to_owned(), JsonValue::Array(citations));
                    anthropic_metadata(meta)
                });
                vec![StreamPart::TextEnd {
                    id: part_id(index),
                    provider_metadata,
                }]
            }
            Block::Reasoning { .. } => vec![StreamPart::ReasoningEnd {
                id: part_id(index),
                provider_metadata: None,
            }],
            Block::ToolCall(block) => {
                let mut input = if block.input.is_empty() {
                    "{}".to_owned()
                } else {
                    block.input
                };
                if block.provider_tool_name.as_deref() == Some("code_execution")
                    && let Ok(parsed) = serde_json::from_str::<JsonValue>(&input)
                {
                    let typed = programmatic_tool_call(parsed);
                    input = typed.to_string();
                }
                let mut call = ToolCall::new(block.tool_call_id.clone(), block.tool_name, input);
                call.provider_executed = block.provider_executed;
                call.dynamic = block.dynamic;
                call.provider_metadata = block.caller;
                vec![
                    StreamPart::ToolInputEnd {
                        id: ToolCallId::new(block.tool_call_id),
                        provider_metadata: None,
                    },
                    StreamPart::ToolCall(call),
                ]
            }
        }
    }

    fn block_delta(&mut self, index: u64, delta: Delta) -> Vec<StreamPart> {
        match delta {
            Delta::Text { text } => {
                if self.mapper.uses_json_response_tool {
                    return Vec::new();
                }
                vec![StreamPart::text_delta(part_id(index), text)]
            }
            Delta::Thinking { thinking } => vec![StreamPart::ReasoningDelta {
                id: part_id(index),
                delta: thinking,
                provider_metadata: None,
            }],
            Delta::Signature { signature } => {
                if !matches!(
                    self.blocks.get(&index),
                    Some(Block::Reasoning { thinking: true })
                ) {
                    return Vec::new();
                }
                let mut meta = JsonObject::new();
                meta.insert("signature".to_owned(), JsonValue::from(signature));
                vec![StreamPart::ReasoningDelta {
                    id: part_id(index),
                    delta: String::new(),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            Delta::Compaction { content } => content
                .map(|content| StreamPart::text_delta(part_id(index), content))
                .into_iter()
                .collect(),
            Delta::InputJson { partial_json } => {
                if partial_json.is_empty() {
                    return Vec::new();
                }
                if self.mapper.json_response_from_tool {
                    return match self.blocks.get(&index) {
                        Some(Block::Text { .. }) => {
                            vec![StreamPart::text_delta(part_id(index), partial_json)]
                        }
                        _ => Vec::new(),
                    };
                }
                let Some(Block::ToolCall(block)) = self.blocks.get_mut(&index) else {
                    return Vec::new();
                };
                let delta = match (block.first_delta, block.provider_tool_input_type) {
                    (true, Some(input_type)) => {
                        format!("{{\"type\": \"{input_type}\",{}", &partial_json[1..])
                    }
                    _ => partial_json,
                };
                block.input.push_str(&delta);
                block.first_delta = false;
                vec![StreamPart::ToolInputDelta {
                    id: ToolCallId::new(block.tool_call_id.clone()),
                    delta,
                    provider_metadata: None,
                }]
            }
            Delta::Citations { citation } => {
                if let Some(Block::Text { citations }) = self.blocks.get_mut(&index)
                    && citation.get("type").and_then(JsonValue::as_str)
                        == Some("web_search_result_location")
                {
                    citations.push(citation.clone());
                }
                self.mapper
                    .citation_source(&citation)
                    .map(StreamPart::Source)
                    .into_iter()
                    .collect()
            }
            Delta::Unknown => Vec::new(),
        }
    }
}

fn non_empty_input(input: Option<&JsonValue>) -> String {
    match input {
        Some(JsonValue::Object(object)) if !object.is_empty() => {
            JsonValue::Object(object.clone()).to_string()
        }
        _ => String::new(),
    }
}

impl StreamMachine for AnthropicStreamState {
    type Chunk = AnthropicChunk;

    fn handle(&mut self, chunk: ParseResult<AnthropicChunk>, include_raw: bool) -> Vec<StreamPart> {
        if self.invalid_sequence {
            return Vec::new();
        }
        let mut parts = Vec::new();
        match chunk {
            ParseResult::Ok { value, raw } => {
                if include_raw {
                    parts.push(StreamPart::Raw {
                        raw_value: raw.clone(),
                    });
                }
                parts.extend(self.handle_chunk(value, &raw));
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

    fn finish(self) -> Vec<StreamPart> {
        Vec::new()
    }
}
