//! Conversion of execution results into the output the model receives.

use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::language_model::prompt::ToolResultOutput;

use crate::error::ToolError;
use crate::tool::ModelOutputArgs;
use crate::tool::Tool;

/// How an output should be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ErrorMode {
    /// A successful result.
    #[default]
    None,
    /// An error rendered as text.
    Text,
    /// An error rendered as JSON.
    Json,
}

/// Builds the model-facing output for a tool result.
///
/// Errors become `error-text` / `error-json`; successful outputs go through
/// the tool's `to_model_output` when defined, otherwise strings become
/// `text` and everything else `json`.
#[must_use]
pub fn create_tool_model_output(
    tool: Option<&Tool>,
    tool_call_id: &ToolCallId,
    input: &JsonValue,
    output: &JsonValue,
    error_mode: ErrorMode,
) -> ToolResultOutput {
    match error_mode {
        ErrorMode::Text => return ToolResultOutput::error_text(error_message(output)),
        ErrorMode::Json => return ToolResultOutput::error_json(output.clone()),
        ErrorMode::None => {}
    }
    if let Some(convert) = tool.and_then(Tool::to_model_output) {
        return convert(ModelOutputArgs {
            tool_call_id,
            input,
            output,
        });
    }
    match output {
        JsonValue::String(text) => ToolResultOutput::text(text.clone()),
        other => ToolResultOutput::json(other.clone()),
    }
}

/// Renders a [`ToolError`] as a model-facing output.
#[must_use]
pub fn tool_error_output(error: &ToolError) -> ToolResultOutput {
    match error {
        ToolError::Json { value } => ToolResultOutput::error_json(value.clone()),
        other => ToolResultOutput::error_text(other.to_string()),
    }
}

/// Extracts a message from an error value: strings as-is, objects through
/// their `message` field, `null` as `unknown error`, anything else as JSON.
#[must_use]
pub fn error_message(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "unknown error".to_owned(),
        JsonValue::String(text) => text.clone(),
        JsonValue::Object(map) => match map.get("message") {
            Some(JsonValue::String(message)) => message.clone(),
            _ => value.to_string(),
        },
        other => other.to_string(),
    }
}
