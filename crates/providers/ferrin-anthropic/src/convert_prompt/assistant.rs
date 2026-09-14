//! Assistant-role content: text, reasoning, tool calls and provider tool
//! results.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use serde_json::json;

use super::Converter;
use super::provider_results;
use super::with_cache_control;
use crate::options::ReasoningMetadata;
use crate::options::read_options;

/// `caller` object of a tool use block, from `caller: {type, toolId}` in
/// the part options.
pub(super) fn caller_block(options: Option<&JsonObject>) -> Option<JsonValue> {
    let caller = options?.get("caller")?.as_object()?;
    let kind = caller.get("type")?.as_str()?;
    if kind == "direct" {
        return Some(json!({"type": "direct"}));
    }
    if kind.starts_with("code_execution_") {
        let tool_id = caller
            .get("toolId")
            .or_else(|| caller.get("tool_id"))
            .and_then(JsonValue::as_str)?;
        return Some(json!({"type": kind, "tool_id": tool_id}));
    }
    None
}

fn tool_use_block(converter: &Converter<'_>, call: &ToolCallPart) -> JsonValue {
    let input = match &call.input {
        JsonValue::Object(_) => call.input.clone(),
        other => json!({"rawInvalidInput": other}),
    };
    let mut block = json!({
        "type": "tool_use",
        "id": call.tool_call_id.as_str(),
        "name": call.tool_name.as_str(),
        "input": input,
    });
    if let Some(caller) = caller_block(converter.options(call.provider_options.as_ref()))
        && let Some(object) = block.as_object_mut()
    {
        object.insert("caller".to_owned(), caller);
    }
    block
}

/// Moves `tool_use`/`server_tool_use`/`mcp_tool_use` blocks behind the other
/// blocks of the same segment, where segments are delimited by thinking
/// blocks.
fn move_tool_use_blocks_to_end(blocks: Vec<JsonValue>) -> Vec<JsonValue> {
    let mut out = Vec::with_capacity(blocks.len());
    let mut segment_other = Vec::new();
    let mut segment_tools = Vec::new();
    let flush =
        |out: &mut Vec<JsonValue>, other: &mut Vec<JsonValue>, tools: &mut Vec<JsonValue>| {
            out.append(other);
            out.append(tools);
        };
    for block in blocks {
        let kind = block.get("type").and_then(JsonValue::as_str).unwrap_or("");
        match kind {
            "thinking" | "redacted_thinking" => {
                flush(&mut out, &mut segment_other, &mut segment_tools);
                out.push(block);
            }
            "tool_use" => segment_tools.push(block),
            _ => segment_other.push(block),
        }
    }
    flush(&mut out, &mut segment_other, &mut segment_tools);
    out
}

pub(super) fn convert_assistant_group(
    converter: &mut Converter<'_>,
    group: &[&PromptMessage],
    is_last_group: bool,
) -> Vec<JsonValue> {
    let mut blocks = Vec::new();
    let message_count = group.len();
    for (message_index, message) in group.iter().enumerate() {
        let PromptMessage::Assistant {
            content,
            provider_options,
        } = message
        else {
            continue;
        };
        let is_last_message = message_index + 1 == message_count;
        let part_count = content.len();
        for (index, part) in content.iter().enumerate() {
            let is_last_part = index + 1 == part_count;
            let trim = is_last_group && is_last_message && is_last_part;
            match part {
                AssistantPromptPart::Text(text) => {
                    let cache_control = converter.cache_control(
                        text.provider_options.as_ref(),
                        provider_options.as_ref(),
                        is_last_part,
                        "text",
                        true,
                    );
                    let options = converter.options(text.provider_options.as_ref());
                    let is_compaction = options
                        .and_then(|o| o.get("type"))
                        .and_then(JsonValue::as_str)
                        == Some("compaction");
                    let value = if trim {
                        text.text.trim_end().to_owned()
                    } else {
                        text.text.clone()
                    };
                    if is_compaction {
                        blocks.push(json!({"type": "compaction", "content": value}));
                        continue;
                    }
                    let mut block = json!({"type": "text", "text": value});
                    if let Some(citations) = options.and_then(|o| o.get("citations"))
                        && let Some(object) = block.as_object_mut()
                    {
                        object.insert("citations".to_owned(), citations.clone());
                    }
                    blocks.push(with_cache_control(block, cache_control));
                }
                AssistantPromptPart::Reasoning(reasoning) => {
                    if !converter.send_reasoning {
                        converter.warnings.push(ferrin_spec::Warning::other(
                            "sending reasoning content is disabled for this model",
                        ));
                        continue;
                    }
                    let metadata: Option<ReasoningMetadata> =
                        read_options(converter.options(reasoning.provider_options.as_ref()));
                    // Thinking blocks cannot carry cache control; report it.
                    let _ = converter.cache_control(
                        reasoning.provider_options.as_ref(),
                        None,
                        false,
                        "thinking",
                        false,
                    );
                    match metadata {
                        Some(ReasoningMetadata {
                            signature: Some(signature),
                            ..
                        }) => blocks.push(json!({
                            "type": "thinking",
                            "thinking": reasoning.text,
                            "signature": signature,
                        })),
                        Some(ReasoningMetadata {
                            redacted_data: Some(data),
                            ..
                        }) => blocks.push(json!({"type": "redacted_thinking", "data": data})),
                        _ => converter.warnings.push(ferrin_spec::Warning::other(
                            "unsupported reasoning metadata",
                        )),
                    }
                }
                AssistantPromptPart::ToolCall(call) => {
                    if call.provider_executed {
                        if let Some(block) = provider_results::provider_tool_use(converter, call) {
                            blocks.push(block);
                        }
                        continue;
                    }
                    let cache_control = converter.cache_control(
                        call.provider_options.as_ref(),
                        provider_options.as_ref(),
                        is_last_part,
                        "tool call",
                        true,
                    );
                    blocks.push(with_cache_control(
                        tool_use_block(converter, call),
                        cache_control,
                    ));
                }
                AssistantPromptPart::ToolResult(result) => {
                    if let Some(block) = provider_results::provider_tool_result(converter, result) {
                        blocks.push(block);
                    }
                }
                AssistantPromptPart::File(_) => converter.warn_unsupported("assistant file parts"),
                AssistantPromptPart::ReasoningFile(_) => {
                    converter.warn_unsupported("assistant reasoning file parts");
                }
                AssistantPromptPart::Custom(custom) => {
                    converter.warn_unsupported(format!("assistant custom part: {}", custom.kind))
                }
                #[allow(unreachable_patterns, reason = "AssistantPromptPart is non-exhaustive")]
                _ => converter.warn_unsupported("assistant prompt part"),
            }
        }
    }
    move_tool_use_blocks_to_end(blocks)
}
