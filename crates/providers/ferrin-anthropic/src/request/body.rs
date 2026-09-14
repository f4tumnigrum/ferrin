//! Wire conversion of nested provider options (context management,
//! container, MCP servers).

use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use crate::config::AnthropicConfig;
use crate::options::ContainerSkill;
use crate::options::ContextEdit;
use crate::options::McpServer;

pub(super) fn context_edit(edit: &ContextEdit) -> JsonValue {
    let mut object = JsonObject::new();
    match edit {
        ContextEdit::ClearToolUses {
            trigger,
            keep,
            clear_at_least,
            clear_tool_inputs,
            exclude_tools,
        } => {
            object.insert(
                "type".to_owned(),
                JsonValue::from("clear_tool_uses_20250919"),
            );
            if let Some(trigger) = trigger {
                object.insert("trigger".to_owned(), trigger.clone());
            }
            if let Some(keep) = keep {
                object.insert("keep".to_owned(), keep.clone());
            }
            if let Some(clear_at_least) = clear_at_least {
                object.insert("clear_at_least".to_owned(), clear_at_least.clone());
            }
            if let Some(clear_tool_inputs) = clear_tool_inputs {
                object.insert(
                    "clear_tool_inputs".to_owned(),
                    JsonValue::Bool(*clear_tool_inputs),
                );
            }
            if let Some(exclude_tools) = exclude_tools {
                object.insert("exclude_tools".to_owned(), json!(exclude_tools));
            }
        }
        ContextEdit::ClearThinking { keep } => {
            object.insert(
                "type".to_owned(),
                JsonValue::from("clear_thinking_20251015"),
            );
            if let Some(keep) = keep {
                object.insert("keep".to_owned(), keep.clone());
            }
        }
        ContextEdit::Compact {
            trigger,
            pause_after_compaction,
            instructions,
        } => {
            object.insert("type".to_owned(), JsonValue::from("compact_20260112"));
            if let Some(trigger) = trigger {
                object.insert("trigger".to_owned(), trigger.clone());
            }
            if let Some(pause) = pause_after_compaction {
                object.insert("pause_after_compaction".to_owned(), JsonValue::Bool(*pause));
            }
            if let Some(instructions) = instructions {
                object.insert(
                    "instructions".to_owned(),
                    JsonValue::from(instructions.clone()),
                );
            }
        }
    }
    JsonValue::Object(object)
}

pub(super) fn container_value(
    config: &AnthropicConfig,
    container: &crate::options::Container,
) -> Result<Option<JsonValue>, ProviderError> {
    match &container.skills {
        Some(skills) if !skills.is_empty() => {
            let mut converted = Vec::with_capacity(skills.len());
            for skill in skills {
                let (kind, skill_id, version) = match skill {
                    ContainerSkill::Anthropic { skill_id, version } => {
                        ("anthropic", skill_id.clone(), version.clone())
                    }
                    ContainerSkill::Custom {
                        provider_reference,
                        version,
                    } => (
                        "custom",
                        resolve_provider_reference(provider_reference, &config.name)?.to_owned(),
                        version.clone(),
                    ),
                };
                let mut object = JsonObject::new();
                object.insert("type".to_owned(), JsonValue::from(kind));
                object.insert("skill_id".to_owned(), JsonValue::from(skill_id));
                if let Some(version) = version {
                    object.insert("version".to_owned(), JsonValue::from(version));
                }
                converted.push(JsonValue::Object(object));
            }
            let mut object = JsonObject::new();
            if let Some(id) = &container.id {
                object.insert("id".to_owned(), JsonValue::from(id.clone()));
            }
            object.insert("skills".to_owned(), JsonValue::Array(converted));
            Ok(Some(JsonValue::Object(object)))
        }
        _ => Ok(container.id.clone().map(JsonValue::from)),
    }
}

pub(super) fn has_code_execution_tool(tools: &[ToolDefinition]) -> bool {
    tools.iter().any(|tool| {
        matches!(
            tool,
            ToolDefinition::Provider { id, .. }
                if id == "anthropic.code_execution_20250825" || id == "anthropic.code_execution_20260120"
        )
    })
}

/// Converts MCP server options to the wire format.
pub(super) fn mcp_servers_value(servers: &[McpServer]) -> Vec<JsonValue> {
    servers
        .iter()
        .map(|server| {
            let mut object = JsonObject::new();
            object.insert("type".to_owned(), JsonValue::from(server.kind.clone()));
            object.insert("name".to_owned(), JsonValue::from(server.name.clone()));
            object.insert("url".to_owned(), JsonValue::from(server.url.clone()));
            if let Some(token) = &server.authorization_token {
                object.insert(
                    "authorization_token".to_owned(),
                    JsonValue::from(token.clone()),
                );
            }
            if let Some(tool_config) = &server.tool_configuration {
                let mut configuration = JsonObject::new();
                if let Some(allowed) = &tool_config.allowed_tools {
                    configuration.insert("allowed_tools".to_owned(), json!(allowed));
                }
                if let Some(enabled) = tool_config.enabled {
                    configuration.insert("enabled".to_owned(), JsonValue::Bool(enabled));
                }
                object.insert(
                    "tool_configuration".to_owned(),
                    JsonValue::Object(configuration),
                );
            }
            JsonValue::Object(object)
        })
        .collect()
}
