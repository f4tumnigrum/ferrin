//! Conversion of tool messages (results and approval responses) to
//! Responses API input items.
//!
//! Advanced tool behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use std::collections::HashSet;

use ferrin_provider_util::media_type::resolve_full_media_type;
use ferrin_spec::FileData;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use serde_json::json;

use super::convert_prompt::ConversionContext;
use super::convert_prompt::ConvertedInput;
use super::convert_prompt::data_url;
use super::convert_prompt::item_reference;
use super::convert_prompt::part_options;

/// Message used when execution was denied without a reason.
pub const EXECUTION_DENIED_MESSAGE: &str = "Tool call execution denied.";

pub(super) fn convert_tool_message(
    content: &[ToolPromptPart],
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<(), ProviderError> {
    let mut processed_approvals: HashSet<String> = HashSet::new();
    for part in content {
        match part {
            ToolPromptPart::ToolApprovalResponse(response) => {
                let approval_id = response.approval_id.as_str();
                if !processed_approvals.insert(approval_id.to_owned()) {
                    continue;
                }
                if ctx.store && !ctx.has_conversation && !ctx.has_previous_response_id {
                    out.input.push(item_reference(approval_id));
                }
                out.input.push(json!({
                    "type": "mcp_approval_response",
                    "approval_request_id": approval_id,
                    "approve": response.approved,
                }));
            }
            ToolPromptPart::ToolResult(result) => convert_tool_result(result, ctx, out)?,
            #[allow(unreachable_patterns, reason = "ToolPromptPart is non-exhaustive")]
            _ => out
                .warnings
                .push(Warning::unsupported("tool prompt part type")),
        }
    }
    Ok(())
}

fn convert_tool_result(
    result: &ToolResultPart,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<(), ProviderError> {
    if let ToolResultOutput::ExecutionDenied {
        provider_options, ..
    } = &result.output
        && provider_options
            .as_ref()
            .and_then(|options| options.get(ctx.provider_options_key))
            .and_then(|options| options.get("approvalId"))
            .is_some()
    {
        // Already sent as an approval response.
        return Ok(());
    }
    let provider_name = ctx
        .tool_name_mapping
        .to_provider_tool_name(result.tool_name.as_str());
    let call_id = result.tool_call_id.as_str();
    if ctx.provider_tools.apply_patch
        && provider_name == "apply_patch"
        && let ToolResultOutput::Json { value, .. } = &result.output
    {
        out.input.push(json!({
            "type": "apply_patch_call_output",
            "call_id": call_id,
            "status": value.get("status").cloned().unwrap_or_else(|| JsonValue::from("completed")),
            "output": value.get("output").cloned().unwrap_or(JsonValue::Null),
        }));
        return Ok(());
    }
    if ctx.provider_tools.computer
        && provider_name == "computer"
        && let ToolResultOutput::Json { value, .. } = &result.output
    {
        let output = value.get("output").cloned().unwrap_or(JsonValue::Null);
        let mut item = json!({
            "type": "computer_call_output",
            "call_id": call_id,
            "output": {
                "type": "computer_screenshot",
                "image_url": output.get("imageUrl").cloned().unwrap_or(JsonValue::Null),
                "file_id": output.get("fileId").cloned().unwrap_or(JsonValue::Null),
                "detail": output.get("detail").cloned().unwrap_or(JsonValue::Null),
            },
        });
        if let Some(checks) = value.get("acknowledgedSafetyChecks")
            && let Some(object) = item.as_object_mut()
        {
            object.insert("acknowledged_safety_checks".to_owned(), checks.clone());
        }
        out.input.push(item);
        return Ok(());
    }
    if ctx.provider_tools.local_shell && provider_name == "local_shell" {
        let output = match &result.output {
            ToolResultOutput::Json { value, .. } | ToolResultOutput::ErrorJson { value, .. } => {
                value.get("output").cloned().unwrap_or(JsonValue::Null)
            }
            _ => convert_output(&result.output, ctx, out)?,
        };
        if !output.is_string() {
            return Err(UnsupportedFunctionalityError::with_message(
                "local shell tool result output",
                "local shell tool results require a string output",
            )
            .into());
        }
        out.input.push(json!({
            "type": "local_shell_call_output",
            "call_id": call_id,
            "output": output,
        }));
        return Ok(());
    }
    if ctx.provider_tools.shell && provider_name == "shell" {
        let value = match &result.output {
            ToolResultOutput::Json { value, .. } | ToolResultOutput::ErrorJson { value, .. } => {
                value
            }
            _ => {
                return Err(UnsupportedFunctionalityError::with_message(
                    "shell tool result output",
                    "shell tool results require a JSON output array",
                )
                .into());
            }
        };
        let Some(mut output) = value.get("output").and_then(JsonValue::as_array).cloned() else {
            return Err(UnsupportedFunctionalityError::with_message(
                "shell tool result output",
                "shell tool results require a JSON output array",
            )
            .into());
        };
        for entry in &mut output {
            if let Some(outcome) = entry.get_mut("outcome").and_then(JsonValue::as_object_mut)
                && let Some(exit_code) = outcome.remove("exitCode")
            {
                outcome.insert("exit_code".to_owned(), exit_code);
            }
        }
        out.input.push(json!({
            "type": "shell_call_output",
            "call_id": call_id,
            "output": output,
        }));
        return Ok(());
    }
    if ctx.provider_tools.tool_search
        && provider_name == "tool_search"
        && let ToolResultOutput::Json { value, .. } = &result.output
    {
        out.input.push(json!({"type": "tool_search_output", "execution": "client", "call_id": call_id, "status": "completed", "tools": value.get("tools")}));
        return Ok(());
    }
    let options = part_options(result.provider_options.as_ref(), ctx.provider_options_key)?;
    if matches!(result.output, ToolResultOutput::ExecutionDenied { .. })
        && options
            .caller
            .as_ref()
            .and_then(|caller| caller.get("type"))
            .and_then(JsonValue::as_str)
            == Some("program")
    {
        return Err(UnsupportedFunctionalityError::new(
            "execution-denied results for programmatic tool calls",
        )
        .into());
    }
    let mut output = convert_output(&result.output, ctx, out)?;
    if ctx
        .provider_tools
        .output_schema_tool_names
        .contains(result.tool_name.as_str())
        && matches!(
            result.output,
            ToolResultOutput::Text { .. }
                | ToolResultOutput::ErrorText { .. }
                | ToolResultOutput::ExecutionDenied { .. }
        )
    {
        output = JsonValue::String(output.to_string());
    }
    let item_type = if ctx.provider_tools.custom_tool_names.contains(provider_name) {
        "custom_tool_call_output"
    } else {
        "function_call_output"
    };
    let mut item = json!({
        "type": item_type,
        "call_id": call_id,
        "output": output,
    });
    if let Some(caller) = &options.caller
        && let Some(object) = item.as_object_mut()
    {
        object.insert(
            "caller".into(),
            super::replay_advanced::caller_to_wire(caller),
        );
    }
    out.input.push(item);
    Ok(())
}

fn convert_output(
    output: &ToolResultOutput,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<JsonValue, ProviderError> {
    Ok(match output {
        ToolResultOutput::Text { value, .. } | ToolResultOutput::ErrorText { value, .. } => {
            JsonValue::from(value.as_str())
        }
        ToolResultOutput::ExecutionDenied { reason, .. } => {
            JsonValue::from(reason.as_deref().unwrap_or(EXECUTION_DENIED_MESSAGE))
        }
        ToolResultOutput::Json { value, .. } | ToolResultOutput::ErrorJson { value, .. } => {
            JsonValue::from(value.to_string())
        }
        ToolResultOutput::Content { value } => {
            let mut parts = Vec::with_capacity(value.len());
            for item in value {
                if let Some(part) = convert_content_part(item, ctx, out)? {
                    parts.push(part);
                }
            }
            JsonValue::Array(parts)
        }
        #[allow(unreachable_patterns, reason = "ToolResultOutput is non-exhaustive")]
        _ => return Err(UnsupportedFunctionalityError::new("tool result output type").into()),
    })
}

fn convert_content_part(
    item: &ToolResultContentPart,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<Option<JsonValue>, ProviderError> {
    match item {
        ToolResultContentPart::Text { text, .. } => {
            Ok(Some(json!({"type": "input_text", "text": text})))
        }
        ToolResultContentPart::File {
            data,
            media_type,
            filename,
            provider_options,
        } => {
            let options = part_options(provider_options.as_ref(), ctx.provider_options_key)?;
            let is_image = media_type.top_level() == "image";
            let with_detail = |mut item: JsonValue| {
                if let Some(detail) = &options.image_detail
                    && let Some(object) = item.as_object_mut()
                {
                    object.insert("detail".to_owned(), JsonValue::from(detail.as_str()));
                }
                item
            };
            match data {
                FileData::Bytes { data: bytes } => {
                    let full = resolve_full_media_type(media_type, Some(bytes))?;
                    let url = data_url(full.as_str(), data).unwrap_or_default();
                    Ok(Some(if is_image {
                        with_detail(json!({"type": "input_image", "image_url": url}))
                    } else {
                        json!({
                            "type": "input_file",
                            "filename": filename.clone().unwrap_or_else(|| "data".to_owned()),
                            "file_data": url,
                        })
                    }))
                }
                FileData::Url { url } => Ok(Some(if is_image {
                    with_detail(json!({"type": "input_image", "image_url": url.to_string()}))
                } else {
                    json!({"type": "input_file", "file_url": url.to_string()})
                })),
                _ => {
                    out.warnings.push(Warning::other(
                        "unsupported tool result content part: file with reference or text data",
                    ));
                    Ok(None)
                }
            }
        }
        _ => {
            out.warnings
                .push(Warning::other("unsupported tool result content part type"));
            Ok(None)
        }
    }
}
