//! Conversion of tool definitions and tool choice.

use ferrin_schema::SchemaTransform;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

/// Converted tools.
#[derive(Debug, Clone, Default)]
pub struct PreparedTools {
    /// `tools` array (`None` when empty).
    pub tools: Option<Vec<JsonValue>>,
    /// `tool_choice`.
    pub tool_choice: Option<JsonValue>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts function tools to `{type: function, function: {..}}` objects;
/// provider tools produce warnings.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for an unsupported
/// tool choice or [`ProviderError::InvalidArgument`] for schemas that cannot
/// be represented in strict mode.
pub fn prepare_tools(
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
) -> Result<PreparedTools, ProviderError> {
    let mut out = PreparedTools::default();
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
                let mut function = JsonObject::new();
                function.insert("name".to_owned(), JsonValue::from(name.as_str()));
                if let Some(description) = description {
                    function.insert(
                        "description".to_owned(),
                        JsonValue::from(description.as_str()),
                    );
                }
                let parameters = if *strict == Some(true) {
                    SchemaTransform::OpenAiStrict.applied(input_schema.clone())?
                } else {
                    input_schema.clone()
                };
                function.insert("parameters".to_owned(), parameters);
                if let Some(strict) = strict {
                    function.insert("strict".to_owned(), JsonValue::Bool(*strict));
                }
                converted.push(json!({"type": "function", "function": function}));
            }
            ToolDefinition::Provider { id, .. } => out
                .warnings
                .push(Warning::unsupported(format!("provider-defined tool {id}"))),
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => out.warnings.push(Warning::unsupported("tool type")),
        }
    }
    out.tools = Some(converted);
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
