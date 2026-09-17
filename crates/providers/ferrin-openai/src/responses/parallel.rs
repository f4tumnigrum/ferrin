//! Internal parallel function wrappers and their replay identity.
//!
//! Protocol behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use std::collections::HashSet;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::ToolCall;
use serde::Deserialize;
use serde_json::json;

use super::api_types::OutputItem;
use super::output::metadata;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ParallelIdentity {
    pub item_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: String,
    pub index: usize,
    pub count: usize,
}

pub(super) fn identity(options: Option<&ProviderOptions>, key: &str) -> Option<ParallelIdentity> {
    let value = options?.get(key)?.get("parallelToolCall")?;
    let identity: ParallelIdentity = serde_json::from_value(value.clone()).ok()?;
    (identity.index < identity.count).then_some(identity)
}

pub(super) fn expand(
    item: &OutputItem,
    functions: &HashSet<String>,
    key: &str,
) -> Option<Vec<Content>> {
    if item.get_str("name") != Some("parallel") || functions.contains("parallel") {
        return None;
    }
    let input = item.get_str("arguments")?;
    let parsed: JsonValue = serde_json::from_str(input).ok()?;
    let uses = parsed.get("tool_uses")?.as_array()?;
    if uses.is_empty() {
        return None;
    }
    uses.iter()
        .enumerate()
        .map(|(index, child)| {
            let name = child
                .get("recipient_name")?
                .as_str()?
                .strip_prefix("functions.")?;
            if !functions.contains(name) {
                return None;
            }
            let parameters = child.get("parameters")?.as_object()?;
            let call_id = item.get_str("call_id")?;
            let mut call = ToolCall::new(
                format!("{call_id}_{index}"),
                name,
                JsonValue::Object(parameters.clone()).to_string(),
            );
            let mut meta = JsonObject::new();
            meta.insert("parallelToolCall".into(), json!({
            "itemId": item.id_str(), "toolCallId": call_id, "toolName": "parallel", "input": input,
            "index": index, "count": uses.len()
        }));
            call.provider_metadata = Some(metadata(key, meta));
            Some(Content::ToolCall(call))
        })
        .collect()
}

/// Reassembles complete child result groups only for server-state continuations.
pub(super) fn regroup(
    prompt: &[ferrin_spec::language_model::PromptMessage],
    ctx: &super::convert_prompt::ConversionContext<'_>,
    out: &mut super::convert_prompt::ConvertedInput,
) {
    use ferrin_spec::language_model::PromptMessage;
    use ferrin_spec::language_model::prompt::ToolPromptPart;
    if !ctx.has_conversation && !ctx.has_previous_response_id {
        return;
    }
    let mut groups: std::collections::BTreeMap<String, Vec<(ParallelIdentity, String)>> =
        std::collections::BTreeMap::new();
    for message in prompt {
        if let PromptMessage::Tool { content, .. } = message {
            for part in content {
                if let ToolPromptPart::ToolResult(result) = part
                    && let Some(meta) =
                        identity(result.provider_options.as_ref(), ctx.provider_options_key)
                {
                    groups
                        .entry(meta.tool_call_id.clone())
                        .or_default()
                        .push((meta, result.tool_call_id.as_str().to_owned()));
                }
            }
        }
    }
    for (wrapper_id, mut children) in groups {
        children.sort_by_key(|(meta, _)| meta.index);
        let Some((first, _)) = children.first() else {
            continue;
        };
        if children.len() != first.count
            || children.iter().enumerate().any(|(index, (meta, _))| {
                meta.index != index
                    || meta.count != first.count
                    || meta.item_id != first.item_id
                    || meta.input != first.input
                    || meta.tool_name != first.tool_name
            })
        {
            continue;
        }
        let outputs: Option<Vec<String>> = children
            .iter()
            .map(|(_, child_id)| {
                let value = out
                    .input
                    .iter()
                    .find(|item| {
                        item.get("type").and_then(JsonValue::as_str) == Some("function_call_output")
                            && item.get("call_id").and_then(JsonValue::as_str)
                                == Some(child_id.as_str())
                    })?
                    .get("output")?;
                Some(
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned),
                )
            })
            .collect();
        let Some(outputs) = outputs else {
            continue;
        };
        let mut call_emitted = false;
        let mut result_emitted = false;
        let previous = std::mem::take(&mut out.input);
        for item in previous {
            let child = item
                .get("call_id")
                .and_then(JsonValue::as_str)
                .is_some_and(|id| children.iter().any(|(_, child_id)| child_id == id));
            if !child {
                out.input.push(item);
                continue;
            }
            match item.get("type").and_then(JsonValue::as_str) {
                Some("function_call") => {
                    if !call_emitted && !ctx.has_conversation {
                        out.input.push(json!({"type": "function_call", "call_id": wrapper_id, "name": first.tool_name, "arguments": first.input}));
                    }
                    call_emitted = true;
                }
                Some("function_call_output") => {
                    if !result_emitted {
                        out.input.push(json!({"type": "function_call_output", "call_id": wrapper_id, "output": outputs.join("\n")}));
                    }
                    result_emitted = true;
                }
                _ => out.input.push(item),
            }
        }
    }
}
