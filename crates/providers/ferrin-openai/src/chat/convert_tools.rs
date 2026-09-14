//! Conversion of tool definitions and tool choice for the Chat Completions
//! API.

use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use crate::json_schema::normalize_json_schema;

/// Converted tools.
#[derive(Debug, Clone, Default)]
pub struct ConvertedTools {
    /// `tools` array (`None` when empty).
    pub tools: Option<Vec<JsonValue>>,
    /// `tool_choice`.
    pub tool_choice: Option<JsonValue>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts the tools and the tool choice.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for an unsupported tool
/// choice or an unsupported JSON schema construct.
pub fn convert_tools(
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
    strict_json_schema: bool,
) -> Result<ConvertedTools, ProviderError> {
    let mut out = ConvertedTools::default();
    if tools.is_empty() {
        return Ok(out);
    }
    let mut converted = Vec::with_capacity(tools.len());
    for tool in tools {
        match tool {
            ToolDefinition::Function {
                name,
                description,
                input_schema,
                strict,
                ..
            } => {
                let (parameters, schema_warnings) = normalize_json_schema(input_schema)?;
                out.warnings.extend(schema_warnings);
                let mut inner = json!({
                    "name": name,
                    "parameters": parameters,
                });
                if let Some(object) = inner.as_object_mut() {
                    if let Some(description) = description {
                        object.insert(
                            "description".to_owned(),
                            JsonValue::from(description.as_str()),
                        );
                    }
                    if strict.unwrap_or(strict_json_schema) {
                        object.insert("strict".to_owned(), JsonValue::Bool(true));
                    }
                }
                converted.push(json!({"type": "function", "function": inner}));
            }
            ToolDefinition::Provider { id, .. } => {
                out.warnings
                    .push(Warning::unsupported(format!("tool type: {id}")));
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => out.warnings.push(Warning::unsupported("tool type")),
        }
    }
    if !converted.is_empty() {
        out.tools = Some(converted);
    }
    out.tool_choice = match tool_choice {
        None => None,
        Some(ToolChoice::Auto) => Some(JsonValue::from("auto")),
        Some(ToolChoice::None) => Some(JsonValue::from("none")),
        Some(ToolChoice::Required) => Some(JsonValue::from("required")),
        Some(ToolChoice::Tool { tool_name }) => Some(json!({
            "type": "function",
            "function": {"name": tool_name},
        })),
        #[allow(unreachable_patterns, reason = "ToolChoice is non-exhaustive")]
        Some(_) => return Err(UnsupportedFunctionalityError::new("tool choice type").into()),
    };
    Ok(out)
}
