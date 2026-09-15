//! Conversion of tool definitions and tool choice for the Responses API.

use std::collections::HashMap;

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use super::convert_prompt::ProviderToolSet;
use super::options::AllowedToolsOptions;
use super::options::FunctionToolOptions;
use crate::json_schema::normalize_json_schema;

/// Provider tool ids and their Responses API type names.
pub const PROVIDER_TOOL_TYPES: &[(&str, &str)] = &[
    ("openai.code_interpreter", "code_interpreter"),
    ("openai.computer", "computer"),
    ("openai.file_search", "file_search"),
    ("openai.image_generation", "image_generation"),
    ("openai.local_shell", "local_shell"),
    ("openai.shell", "shell"),
    ("openai.web_search", "web_search"),
    ("openai.web_search_preview", "web_search_preview"),
    ("openai.mcp", "mcp"),
    ("openai.apply_patch", "apply_patch"),
    ("openai.tool_search", "tool_search"),
    (
        "openai.programmatic_tool_calling",
        "programmatic_tool_calling",
    ),
    ("openai.custom", "custom"),
];

/// Builds the tool name mapping (custom name ↔ provider name).
#[must_use]
pub fn tool_name_mapping(tools: &[ToolDefinition]) -> ToolNameMapping {
    let names: HashMap<&str, &str> = PROVIDER_TOOL_TYPES
        .iter()
        .copied()
        .filter(|(id, _)| *id != "openai.custom")
        .collect();
    let mut mapping = ToolNameMapping::new(tools, &names);
    for tool in tools {
        if let ToolDefinition::Provider { id, name, args } = tool
            && id == "openai.custom"
        {
            let provider_name = args
                .get("name")
                .and_then(JsonValue::as_str)
                .unwrap_or(name.as_str());
            mapping = mapping.with_pair(name.as_str(), provider_name);
        }
    }
    mapping
}

/// Converted tools.
#[derive(Debug, Clone, Default)]
pub struct ConvertedTools {
    /// `tools` array (`None` when empty).
    pub tools: Option<Vec<JsonValue>>,
    /// `tool_choice` value.
    pub tool_choice: Option<JsonValue>,
    /// Provider-defined tools present.
    pub provider_tools: ProviderToolSet,
    /// Name of the declared web search tool (custom name), if any.
    pub web_search_tool_name: Option<String>,
    /// Whether a web search tool is declared.
    pub has_web_search: bool,
    /// Whether a code interpreter tool is declared.
    pub has_code_interpreter: bool,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts tools and tool choice.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] for invalid tool options and
/// [`ProviderError::UnsupportedFunctionality`] for unsupported schemas.
pub fn convert_tools(
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
    mapping: &ToolNameMapping,
    strict_json_schema: bool,
    provider_options_key: &str,
) -> Result<ConvertedTools, ProviderError> {
    let mut out = ConvertedTools::default();
    let mut converted: Vec<JsonValue> = Vec::new();
    let mut namespaces: Vec<(String, Option<String>, Vec<JsonValue>)> = Vec::new();
    for tool in tools {
        match tool {
            ToolDefinition::Function {
                name,
                description,
                input_schema,
                strict,
                provider_options,
                ..
            } => {
                let options = provider_options
                    .as_ref()
                    .map(|options| {
                        parse_provider_options::<FunctionToolOptions>(provider_options_key, options)
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or_default();
                let (parameters, warnings) = normalize_json_schema(input_schema)?;
                out.warnings.extend(warnings);
                let mut function = JsonObject::new();
                function.insert("type".to_owned(), JsonValue::from("function"));
                function.insert("name".to_owned(), JsonValue::from(name.as_str()));
                function.insert(
                    "description".to_owned(),
                    description.clone().map_or(JsonValue::Null, JsonValue::from),
                );
                function.insert("parameters".to_owned(), parameters);
                function.insert(
                    "strict".to_owned(),
                    JsonValue::from(strict.unwrap_or(strict_json_schema)),
                );
                if let Some(is_async) = options.r#async {
                    function.insert("async".to_owned(), JsonValue::from(is_async));
                }
                if let Some(defer) = options.defer_loading {
                    function.insert("defer_loading".to_owned(), JsonValue::from(defer));
                }
                if let Some(callers) = &options.allowed_callers {
                    function.insert("allowed_callers".to_owned(), json!(callers));
                }
                if let Some(schema) = &options.output_schema {
                    function.insert("output_schema".to_owned(), schema.clone());
                }
                let item = JsonValue::Object(function);
                match options.namespace {
                    Some(namespace) => {
                        if let Some(entry) = namespaces.iter_mut().find(|(n, _, _)| *n == namespace)
                        {
                            entry.2.push(item);
                        } else {
                            namespaces.push((namespace, options.namespace_description, vec![item]));
                        }
                    }
                    None => converted.push(item),
                }
            }
            ToolDefinition::Provider { id, name, args } => {
                let Some(provider_type) = provider_tool_type(id) else {
                    out.warnings
                        .push(Warning::unsupported(format!("provider tool {id}")));
                    continue;
                };
                match provider_type {
                    "apply_patch" => out.provider_tools.apply_patch = true,
                    "local_shell" => out.provider_tools.local_shell = true,
                    "shell" => out.provider_tools.shell = true,
                    "computer" => out.provider_tools.computer = true,
                    "custom" => {
                        let custom_name = args
                            .get("name")
                            .and_then(JsonValue::as_str)
                            .unwrap_or(name.as_str())
                            .to_owned();
                        out.provider_tools.custom_tool_names.insert(custom_name);
                    }
                    "web_search" | "web_search_preview" => {
                        out.has_web_search = true;
                        out.web_search_tool_name = Some(name.as_str().to_owned());
                    }
                    "code_interpreter" => out.has_code_interpreter = true,
                    _ => {}
                }
                converted.push(provider_tool_item(provider_type, name.as_str(), args));
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => out
                .warnings
                .push(Warning::unsupported("tool definition type")),
        }
    }
    for (namespace, description, tools) in namespaces {
        let mut item = json!({"type": "namespace", "name": namespace, "tools": tools});
        if let Some(description) = description
            && let Some(object) = item.as_object_mut()
        {
            object.insert("description".to_owned(), JsonValue::from(description));
        }
        converted.push(item);
    }
    if !converted.is_empty() {
        out.tools = Some(converted);
    }
    out.tool_choice = tool_choice
        .and_then(|choice| convert_tool_choice(choice, tools, mapping, provider_options_key));
    Ok(out)
}

/// Responses API type of a provider tool id.
#[must_use]
pub fn provider_tool_type(id: &str) -> Option<&'static str> {
    PROVIDER_TOOL_TYPES
        .iter()
        .find(|(tool_id, _)| *tool_id == id)
        .map(|(_, provider_type)| *provider_type)
}

/// Converts documented API configuration fields while preserving opaque maps.
fn snake_case_keys(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(object) => JsonValue::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let name = match key.as_str() {
                        "allowedDomains" => "allowed_domains",
                        "blockedDomains" => "blocked_domains",
                        "allowedTools" => "allowed_tools",
                        "readOnly" => "read_only",
                        "toolNames" => "tool_names",
                        "connectorId" => "connector_id",
                        "requireApproval" => "require_approval",
                        "serverDescription" => "server_description",
                        "serverLabel" => "server_label",
                        "serverUrl" => "server_url",
                        "inputFidelity" => "input_fidelity",
                        "inputImageMask" => "input_image_mask",
                        "fileId" => "file_id",
                        "imageUrl" => "image_url",
                        "outputCompression" => "output_compression",
                        "outputFormat" => "output_format",
                        "partialImages" => "partial_images",
                        "containerId" => "container_id",
                        "fileIds" => "file_ids",
                        "memoryLimit" => "memory_limit",
                        "networkPolicy" => "network_policy",
                        "displayWidth" => "display_width",
                        "displayHeight" => "display_height",
                        "displayNumber" => "display_number",
                        other => other,
                    };
                    let converted = match name {
                        "allowed_tools" | "require_approval" | "always" | "never"
                        | "input_image_mask" | "network_policy" => snake_case_keys(value),
                        _ => value.clone(),
                    };
                    (name.to_owned(), converted)
                })
                .collect(),
        ),
        JsonValue::Array(list) => JsonValue::Array(list.iter().map(snake_case_keys).collect()),
        other => other.clone(),
    }
}

fn provider_tool_item(provider_type: &str, name: &str, args: &JsonObject) -> JsonValue {
    let mut item = JsonObject::new();
    item.insert("type".to_owned(), JsonValue::from(provider_type));
    match provider_type {
        "code_interpreter" => {
            let container = match args.get("container") {
                Some(JsonValue::String(id)) => JsonValue::from(id.as_str()),
                Some(JsonValue::Object(object)) => {
                    let mut container = json!({"type": "auto"});
                    if let Some(file_ids) = object.get("fileIds")
                        && let Some(c) = container.as_object_mut()
                    {
                        c.insert("file_ids".to_owned(), file_ids.clone());
                    }
                    container
                }
                _ => json!({"type": "auto"}),
            };
            item.insert("container".to_owned(), container);
        }
        "file_search" => {
            if let Some(ids) = args.get("vectorStoreIds") {
                item.insert("vector_store_ids".to_owned(), ids.clone());
            }
            if let Some(max) = args.get("maxNumResults") {
                item.insert("max_num_results".to_owned(), max.clone());
            }
            if let Some(ranking) = args.get("ranking") {
                let mut options = JsonObject::new();
                if let Some(ranker) = ranking.get("ranker") {
                    options.insert("ranker".to_owned(), ranker.clone());
                }
                if let Some(threshold) = ranking.get("scoreThreshold") {
                    options.insert("score_threshold".to_owned(), threshold.clone());
                }
                item.insert("ranking_options".to_owned(), JsonValue::Object(options));
            }
            if let Some(filters) = args.get("filters") {
                item.insert("filters".to_owned(), filters.clone());
            }
        }
        "web_search" => {
            if let Some(filters) = args.get("filters") {
                item.insert("filters".to_owned(), snake_case_keys(filters));
            }
            if let Some(access) = args.get("externalWebAccess") {
                item.insert("external_web_access".to_owned(), access.clone());
            }
            if let Some(size) = args.get("searchContextSize") {
                item.insert("search_context_size".to_owned(), size.clone());
            }
            if let Some(location) = args.get("userLocation") {
                item.insert("user_location".to_owned(), location.clone());
            }
        }
        "web_search_preview" => {
            if let Some(size) = args.get("searchContextSize") {
                item.insert("search_context_size".to_owned(), size.clone());
            }
            if let Some(location) = args.get("userLocation") {
                item.insert("user_location".to_owned(), location.clone());
            }
        }
        "custom" => {
            item.insert(
                "name".to_owned(),
                args.get("name")
                    .cloned()
                    .unwrap_or_else(|| JsonValue::from(name)),
            );
            for key in ["description", "format"] {
                if let Some(value) = args.get(key) {
                    item.insert(key.to_owned(), value.clone());
                }
            }
        }
        "shell" => {
            if let Some(environment) = args.get("environment") {
                item.insert("environment".to_owned(), snake_case_keys(environment));
            }
        }
        _ => {
            // Convert API option names without inspecting user-defined dictionaries.
            if let JsonValue::Object(object) = snake_case_keys(&JsonValue::Object(args.clone())) {
                for (key, value) in object {
                    item.insert(key, value);
                }
            }
        }
    }
    JsonValue::Object(item)
}

fn convert_tool_choice(
    choice: &ToolChoice,
    tools: &[ToolDefinition],
    mapping: &ToolNameMapping,
    provider_options_key: &str,
) -> Option<JsonValue> {
    match choice {
        ToolChoice::Auto => Some(JsonValue::from("auto")),
        ToolChoice::None => Some(JsonValue::from("none")),
        ToolChoice::Required => Some(JsonValue::from("required")),
        ToolChoice::Tool { tool_name } => {
            let tool = tools.iter().find(|tool| tool.name() == tool_name);
            match tool {
                Some(ToolDefinition::Provider { id, args, .. }) => {
                    let provider_type = provider_tool_type(id)?;
                    if provider_type == "custom" {
                        return Some(json!({
                            "type": "custom",
                            "name": args.get("name").cloned().unwrap_or_else(|| JsonValue::from(tool_name.as_str())),
                        }));
                    }
                    if provider_type == "mcp"
                        && let Some(label) = args.get("serverLabel")
                    {
                        return Some(json!({"type": "mcp", "server_label": label}));
                    }
                    Some(json!({"type": provider_type}))
                }
                Some(ToolDefinition::Function {
                    provider_options, ..
                }) => {
                    let allowed = provider_options
                        .as_ref()
                        .and_then(|options| options.get(provider_options_key))
                        .and_then(|options| options.get("allowedTools"))
                        .and_then(|value| {
                            serde_json::from_value::<AllowedToolsOptions>(value.clone()).ok()
                        });
                    if let Some(allowed) = allowed
                        && !allowed.tools.is_empty()
                    {
                        return Some(json!({
                            "type": "allowed_tools",
                            "mode": allowed.mode.unwrap_or_else(|| "auto".to_owned()),
                            "tools": allowed.tools.iter().map(|name| json!({"type": "function", "name": name})).collect::<Vec<_>>(),
                        }));
                    }
                    Some(json!({
                        "type": "function",
                        "name": mapping.to_provider_tool_name(tool_name.as_str()),
                    }))
                }
                _ => Some(json!({
                    "type": "function",
                    "name": mapping.to_provider_tool_name(tool_name.as_str()),
                })),
            }
        }
        #[allow(unreachable_patterns, reason = "ToolChoice is non-exhaustive")]
        _ => None,
    }
}
