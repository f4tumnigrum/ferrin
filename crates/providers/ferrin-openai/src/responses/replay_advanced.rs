//! Reconstruction of advanced provider calls and results without server storage.
//!
//! Protocol behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonValue;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use serde_json::json;

use super::convert_prompt::ConversionContext;
use super::convert_prompt::ConvertedInput;
use super::convert_prompt::item_reference;
use super::convert_prompt::part_options;
use super::options::PartOptions;

pub(super) fn caller_to_wire(caller: &JsonValue) -> JsonValue {
    if caller.get("type").and_then(JsonValue::as_str) == Some("program") {
        json!({"type": "program", "caller_id": caller.get("callerId")})
    } else {
        caller.clone()
    }
}

pub(super) fn add_function_options(item: &mut JsonValue, options: &PartOptions) {
    if let Some(object) = item.as_object_mut() {
        if let Some(caller) = &options.caller {
            object.insert("caller".into(), caller_to_wire(caller));
        }
        if let Some(value) = options.r#async {
            object.insert("async".into(), value.into());
        }
        if let Some(value) = &options.namespace {
            object.insert("namespace".into(), value.as_str().into());
        }
    }
}

pub(super) fn assistant_call(
    call: &ToolCallPart,
    options: &PartOptions,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<bool, ProviderError> {
    let name = ctx
        .tool_name_mapping
        .to_provider_tool_name(call.tool_name.as_str());
    let advanced = (ctx.provider_tools.programmatic && name == "programmatic_tool_calling")
        || (ctx.provider_tools.tool_search && name == "tool_search")
        || (ctx.provider_tools.shell && name == "shell")
        || (ctx.provider_tools.local_shell && name == "local_shell");
    if !advanced {
        return Ok(false);
    }
    let id = options.item_id.as_deref();
    if (ctx.has_conversation || ctx.has_previous_response_id) && ctx.store && id.is_some() {
        return Ok(true);
    }
    if ctx.store
        && let Some(id) = id
    {
        out.input.push(item_reference(id));
        return Ok(true);
    }
    let mut item = match name {
        "programmatic_tool_calling" => {
            require_string(&call.input, "code")?;
            require_string(&call.input, "fingerprint")?;
            json!({"type": "program", "call_id": call.tool_call_id, "code": call.input.get("code"), "fingerprint": call.input.get("fingerprint")})
        }
        "tool_search" => {
            json!({"type": "tool_search_call", "execution": if call.provider_executed { "server" } else { "client" }, "call_id": call.input.get("call_id"), "status": "completed", "arguments": call.input.get("arguments")})
        }
        "shell" | "local_shell" => {
            let mut action =
                call.input.get("action").cloned().ok_or_else(|| {
                    UnsupportedFunctionalityError::new("shell call without action")
                })?;
            if let Some(object) = action.as_object_mut() {
                for (camel, snake) in [
                    ("timeoutMs", "timeout_ms"),
                    ("maxOutputLength", "max_output_length"),
                    ("workingDirectory", "working_directory"),
                ] {
                    if let Some(value) = object.remove(camel) {
                        object.insert(snake.into(), value);
                    }
                }
            }
            json!({"type": format!("{name}_call"), "call_id": call.tool_call_id, "status": "completed", "action": action})
        }
        _ => return Ok(false),
    };
    if let Some(object) = item.as_object_mut() {
        object.insert("id".into(), id.unwrap_or(call.tool_call_id.as_str()).into());
    }
    out.input.push(item);
    Ok(true)
}

fn require_string(value: &JsonValue, field: &str) -> Result<(), ProviderError> {
    if value.get(field).is_some_and(JsonValue::is_string) {
        Ok(())
    } else {
        Err(
            UnsupportedFunctionalityError::new(format!("program call without string {field}"))
                .into(),
        )
    }
}

pub(super) fn assistant_result(
    result: &ToolResultPart,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<bool, ProviderError> {
    let name = ctx
        .tool_name_mapping
        .to_provider_tool_name(result.tool_name.as_str());
    let advanced = (ctx.provider_tools.programmatic && name == "programmatic_tool_calling")
        || (ctx.provider_tools.tool_search && name == "tool_search")
        || (ctx.provider_tools.shell && name == "shell");
    if !advanced {
        return Ok(false);
    }
    let options = part_options(result.provider_options.as_ref(), ctx.provider_options_key)?;
    let id = options
        .item_id
        .as_deref()
        .unwrap_or(result.tool_call_id.as_str());
    if name != "shell" && ctx.store {
        if !ctx.has_previous_response_id {
            out.input.push(item_reference(id));
        }
        return Ok(true);
    }
    let ToolResultOutput::Json { value, .. } = &result.output else {
        return Err(
            UnsupportedFunctionalityError::new("non-JSON advanced provider tool result").into(),
        );
    };
    let item = match name {
        "programmatic_tool_calling" => {
            json!({"type": "program_output", "id": id, "call_id": result.tool_call_id, "result": value.get("result"), "status": value.get("status")})
        }
        "tool_search" => {
            json!({"type": "tool_search_output", "id": id, "execution": "server", "call_id": null, "status": "completed", "tools": value.get("tools")})
        }
        "shell" => {
            json!({"type": "shell_call_output", "call_id": result.tool_call_id, "output": shell_output(value)?})
        }
        _ => return Ok(false),
    };
    out.input.push(item);
    Ok(true)
}

pub(super) fn shell_output(value: &JsonValue) -> Result<JsonValue, ProviderError> {
    let Some(mut entries) = value.get("output").and_then(JsonValue::as_array).cloned() else {
        return Err(
            UnsupportedFunctionalityError::new("shell tool result without output array").into(),
        );
    };
    for entry in &mut entries {
        if let Some(outcome) = entry.get_mut("outcome").and_then(JsonValue::as_object_mut)
            && let Some(code) = outcome.remove("exitCode")
        {
            outcome.insert("exit_code".into(), code);
        }
    }
    Ok(JsonValue::Array(entries))
}

pub(super) fn validate_program_denials(
    prompt: &[ferrin_spec::language_model::PromptMessage],
    key: &str,
) -> Result<(), ProviderError> {
    use ferrin_spec::language_model::PromptMessage;
    use ferrin_spec::language_model::prompt::AssistantPromptPart;
    use ferrin_spec::language_model::prompt::ToolPromptPart;
    let mut program_calls = std::collections::HashSet::new();
    for message in prompt {
        if let PromptMessage::Assistant { content, .. } = message {
            for part in content {
                if let AssistantPromptPart::ToolCall(call) = part {
                    let options = part_options(call.provider_options.as_ref(), key)?;
                    if options
                        .caller
                        .as_ref()
                        .and_then(|caller| caller.get("type"))
                        .and_then(JsonValue::as_str)
                        == Some("program")
                    {
                        program_calls.insert(call.tool_call_id.as_str());
                    }
                }
            }
        }
    }
    for message in prompt {
        if let PromptMessage::Tool { content, .. } = message {
            for part in content {
                if let ToolPromptPart::ToolResult(result) = part
                    && matches!(result.output, ToolResultOutput::ExecutionDenied { .. })
                    && program_calls.contains(result.tool_call_id.as_str())
                {
                    return Err(UnsupportedFunctionalityError::new(
                        "execution-denied results for programmatic tool calls",
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}
