//! Conversion of tool definitions and tool choice to the Messages API.

use std::collections::BTreeSet;
use std::collections::HashMap;

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use crate::cache_control::CacheControlValidator;
use crate::config::AnthropicConfig;
use crate::options::ToolOptions;
use crate::options::part_options;
use crate::options::read_options;

/// Beta enabling native structured outputs and strict tools.
pub const BETA_STRUCTURED_OUTPUTS: &str = "structured-outputs-2025-11-13";
/// Beta enabling `input_examples` and `allowed_callers`.
pub const BETA_ADVANCED_TOOL_USE: &str = "advanced-tool-use-2025-11-20";

/// Provider tool id (`anthropic.<tool>`) → wire tool name, used for
/// [`ToolNameMapping`].
pub const PROVIDER_TOOL_NAMES: &[(&str, &str)] = &[
    ("anthropic.code_execution_20250522", "code_execution"),
    ("anthropic.code_execution_20250825", "code_execution"),
    ("anthropic.code_execution_20260120", "code_execution"),
    ("anthropic.computer_20241022", "computer"),
    ("anthropic.computer_20250124", "computer"),
    ("anthropic.computer_20251124", "computer"),
    ("anthropic.text_editor_20241022", "str_replace_editor"),
    ("anthropic.text_editor_20250124", "str_replace_editor"),
    (
        "anthropic.text_editor_20250429",
        "str_replace_based_edit_tool",
    ),
    (
        "anthropic.text_editor_20250728",
        "str_replace_based_edit_tool",
    ),
    ("anthropic.bash_20241022", "bash"),
    ("anthropic.bash_20250124", "bash"),
    ("anthropic.memory_20250818", "memory"),
    ("anthropic.web_fetch_20250910", "web_fetch"),
    ("anthropic.web_fetch_20260209", "web_fetch"),
    ("anthropic.web_search_20250305", "web_search"),
    ("anthropic.web_search_20260209", "web_search"),
    (
        "anthropic.tool_search_regex_20251119",
        "tool_search_tool_regex",
    ),
    (
        "anthropic.tool_search_bm25_20251119",
        "tool_search_tool_bm25",
    ),
    ("anthropic.advisor_20260301", "advisor"),
];

/// Builds the tool name mapping of a request.
#[must_use]
pub fn tool_name_mapping(tools: &[ToolDefinition]) -> ToolNameMapping {
    let names: HashMap<&str, &str> = PROVIDER_TOOL_NAMES.iter().copied().collect();
    ToolNameMapping::new(tools, &names)
}

/// Converted tools.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreparedTools {
    /// `tools`; `None` when no tools are sent.
    pub tools: Option<Vec<JsonValue>>,
    /// `tool_choice`.
    pub tool_choice: Option<JsonValue>,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// Beta flags required by the tools.
    pub betas: BTreeSet<String>,
}

/// Settings of [`prepare_tools`].
#[derive(Debug, Clone, Copy)]
pub struct PrepareToolsSettings {
    /// Disables parallel tool use for `auto` and `any` choices.
    pub disable_parallel_tool_use: bool,
    /// Whether `output_config.format` is available (enables `strict`).
    pub supports_structured_output: bool,
    /// Whether function tools may carry `strict`.
    pub supports_strict_tools: bool,
    /// Default `eager_input_streaming` for function tools without an
    /// explicit option.
    pub default_eager_input_streaming: bool,
}

fn arg<'a>(args: &'a JsonObject, key: &str) -> Option<&'a JsonValue> {
    args.get(key).filter(|value| !value.is_null())
}

fn insert_arg(tool: &mut JsonObject, args: &JsonObject, key: &str, wire: &str) {
    if let Some(value) = arg(args, key) {
        tool.insert(wire.to_owned(), value.clone());
    }
}

fn provider_tool(id: &str, args: &JsonObject, betas: &mut BTreeSet<String>) -> Option<JsonValue> {
    let wire_name = PROVIDER_TOOL_NAMES
        .iter()
        .find(|(tool_id, _)| *tool_id == id)
        .map(|(_, name)| *name)?;
    let kind = id.strip_prefix("anthropic.")?;
    let wire_type = match kind {
        "tool_search_regex_20251119" => "tool_search_tool_regex_20251119",
        "tool_search_bm25_20251119" => "tool_search_tool_bm25_20251119",
        other => other,
    };
    let mut tool = JsonObject::new();
    tool.insert("type".to_owned(), JsonValue::from(wire_type));
    tool.insert("name".to_owned(), JsonValue::from(wire_name));
    match kind {
        "code_execution_20250522" => {
            betas.insert("code-execution-2025-05-22".to_owned());
        }
        "code_execution_20250825" => {
            betas.insert("code-execution-2025-08-25".to_owned());
        }
        "code_execution_20260120" | "text_editor_20250728" => {}
        "computer_20241022" | "computer_20250124" | "computer_20251124" => {
            let beta = match kind {
                "computer_20241022" => "computer-use-2024-10-22",
                "computer_20250124" => "computer-use-2025-01-24",
                _ => "computer-use-2025-11-24",
            };
            betas.insert(beta.to_owned());
            insert_arg(&mut tool, args, "displayWidthPx", "display_width_px");
            insert_arg(&mut tool, args, "displayHeightPx", "display_height_px");
            insert_arg(&mut tool, args, "displayNumber", "display_number");
            if kind == "computer_20251124" {
                insert_arg(&mut tool, args, "enableZoom", "enable_zoom");
            }
        }
        "text_editor_20241022" | "bash_20241022" => {
            betas.insert("computer-use-2024-10-22".to_owned());
        }
        "text_editor_20250124" | "text_editor_20250429" | "bash_20250124" => {
            betas.insert("computer-use-2025-01-24".to_owned());
        }
        "memory_20250818" => {
            betas.insert("context-management-2025-06-27".to_owned());
        }
        "web_fetch_20250910" | "web_fetch_20260209" => {
            betas.insert(
                if kind == "web_fetch_20250910" {
                    "web-fetch-2025-09-10"
                } else {
                    "code-execution-web-tools-2026-02-09"
                }
                .to_owned(),
            );
            insert_arg(&mut tool, args, "maxUses", "max_uses");
            insert_arg(&mut tool, args, "allowedDomains", "allowed_domains");
            insert_arg(&mut tool, args, "blockedDomains", "blocked_domains");
            insert_arg(&mut tool, args, "citations", "citations");
            insert_arg(&mut tool, args, "maxContentTokens", "max_content_tokens");
        }
        "web_search_20250305" | "web_search_20260209" => {
            if kind == "web_search_20260209" {
                betas.insert("code-execution-web-tools-2026-02-09".to_owned());
            }
            insert_arg(&mut tool, args, "maxUses", "max_uses");
            insert_arg(&mut tool, args, "allowedDomains", "allowed_domains");
            insert_arg(&mut tool, args, "blockedDomains", "blocked_domains");
            insert_arg(&mut tool, args, "userLocation", "user_location");
        }
        "tool_search_regex_20251119" | "tool_search_bm25_20251119" => {}
        "advisor_20260301" => {
            betas.insert("advisor-tool-2026-03-01".to_owned());
            insert_arg(&mut tool, args, "model", "model");
            insert_arg(&mut tool, args, "maxUses", "max_uses");
            insert_arg(&mut tool, args, "maxTokens", "max_tokens");
            insert_arg(&mut tool, args, "caching", "caching");
        }
        _ => return None,
    }
    if kind == "text_editor_20250728" {
        insert_arg(&mut tool, args, "maxCharacters", "max_characters");
    }
    Some(JsonValue::Object(tool))
}

/// Converts `tools` and `tool_choice`.
///
/// Function tools become `{name, description, input_schema, ...}`; provider
/// tools (`anthropic.*`) become their typed wire objects and add the beta
/// flags they need; unknown provider tools produce a warning.
#[must_use]
pub fn prepare_tools(
    config: &AnthropicConfig,
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
    settings: PrepareToolsSettings,
    cache: &mut CacheControlValidator,
) -> PreparedTools {
    let mut prepared = PreparedTools::default();
    if tools.is_empty() {
        return prepared;
    }
    let mut converted = Vec::with_capacity(tools.len());
    for tool in tools {
        match tool {
            ToolDefinition::Function {
                name,
                description,
                input_schema,
                strict,
                input_examples,
                provider_options,
            } => {
                let options: ToolOptions =
                    read_options(part_options(config, provider_options.as_ref()))
                        .unwrap_or_default();
                let cache_control = cache.get(config, provider_options.as_ref(), "tool", true);
                let mut object = JsonObject::new();
                object.insert("name".to_owned(), JsonValue::from(name.as_str()));
                if let Some(description) = description {
                    object.insert(
                        "description".to_owned(),
                        JsonValue::from(description.clone()),
                    );
                }
                object.insert("input_schema".to_owned(), input_schema.clone());
                if let Some(cache_control) = cache_control {
                    object.insert("cache_control".to_owned(), cache_control);
                }
                if options
                    .eager_input_streaming
                    .unwrap_or(settings.default_eager_input_streaming)
                {
                    object.insert("eager_input_streaming".to_owned(), JsonValue::Bool(true));
                }
                if settings.supports_structured_output {
                    prepared.betas.insert(BETA_STRUCTURED_OUTPUTS.to_owned());
                }
                if let Some(strict) = strict {
                    if settings.supports_strict_tools {
                        object.insert("strict".to_owned(), JsonValue::Bool(*strict));
                    } else {
                        prepared.warnings.push(Warning::unsupported_with_details(
                            "strict",
                            format!(
                                "Tool '{name}' has strict: {strict}, but strict mode is not supported by this provider. The strict property will be ignored."
                            ),
                        ));
                    }
                }
                if let Some(defer) = options.defer_loading {
                    object.insert("defer_loading".to_owned(), JsonValue::Bool(defer));
                }
                if let Some(callers) = options.allowed_callers {
                    prepared.betas.insert(BETA_ADVANCED_TOOL_USE.to_owned());
                    object.insert("allowed_callers".to_owned(), json!(callers));
                }
                if !input_examples.is_empty() {
                    prepared.betas.insert(BETA_ADVANCED_TOOL_USE.to_owned());
                    object.insert(
                        "input_examples".to_owned(),
                        JsonValue::Array(
                            input_examples
                                .iter()
                                .cloned()
                                .map(JsonValue::Object)
                                .collect(),
                        ),
                    );
                }
                converted.push(JsonValue::Object(object));
            }
            ToolDefinition::Provider { id, name, args } => {
                match provider_tool(id, args, &mut prepared.betas) {
                    Some(tool) => converted.push(tool),
                    None => prepared.warnings.push(Warning::unsupported_with_details(
                        format!("tool: {name}"),
                        format!("provider tool `{id}` is not supported by Anthropic"),
                    )),
                }
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => prepared
                .warnings
                .push(Warning::unsupported("tool definition")),
        }
    }
    match tool_choice {
        None | Some(ToolChoice::Auto) => {
            prepared.tools = Some(converted);
            if tool_choice.is_some() || settings.disable_parallel_tool_use {
                let mut choice = json!({"type": "auto"});
                if settings.disable_parallel_tool_use
                    && let Some(object) = choice.as_object_mut()
                {
                    object.insert(
                        "disable_parallel_tool_use".to_owned(),
                        JsonValue::Bool(true),
                    );
                }
                prepared.tool_choice = Some(choice);
            }
        }
        Some(ToolChoice::None) => {
            // Anthropic has no `none` choice: drop the tools instead.
            prepared.tools = None;
        }
        Some(ToolChoice::Required) => {
            prepared.tools = Some(converted);
            let mut choice = json!({"type": "any"});
            if settings.disable_parallel_tool_use
                && let Some(object) = choice.as_object_mut()
            {
                object.insert(
                    "disable_parallel_tool_use".to_owned(),
                    JsonValue::Bool(true),
                );
            }
            prepared.tool_choice = Some(choice);
        }
        Some(ToolChoice::Tool { tool_name }) => {
            prepared.tools = Some(converted);
            let mut choice = json!({"type": "tool", "name": tool_name.as_str()});
            if settings.disable_parallel_tool_use
                && let Some(object) = choice.as_object_mut()
            {
                object.insert(
                    "disable_parallel_tool_use".to_owned(),
                    JsonValue::Bool(true),
                );
            }
            prepared.tool_choice = Some(choice);
        }
        #[allow(unreachable_patterns, reason = "ToolChoice may grow")]
        Some(_) => {
            prepared.tools = Some(converted);
            prepared.warnings.push(Warning::unsupported("toolChoice"));
        }
    }
    prepared
}
