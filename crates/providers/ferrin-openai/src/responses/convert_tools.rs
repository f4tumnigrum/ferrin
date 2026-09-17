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
    ToolNameMapping::new(tools, &names)
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
/// The legacy `strict_json_schema` argument is ignored; only explicit per-tool
/// strict flags are sent, and schemas retain their supplied constraints.
pub fn convert_tools(
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
    mapping: &ToolNameMapping,
    _strict_json_schema: bool,
    provider_options_key: &str,
) -> Result<ConvertedTools, ProviderError> {
    let mut out = ConvertedTools::default();
    if tools.is_empty() {
        return Ok(out);
    }
    let mut converted: Vec<JsonValue> = Vec::new();
    let mut namespaces: Vec<(String, Option<String>, usize)> = Vec::new();
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
                if let Some(description) = description {
                    function.insert(
                        "description".to_owned(),
                        JsonValue::from(description.as_str()),
                    );
                }
                function.insert("parameters".to_owned(), parameters);
                if let Some(strict) = strict {
                    function.insert("strict".to_owned(), JsonValue::from(*strict));
                }
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
                    let (schema, warnings) = normalize_json_schema(schema)?;
                    out.warnings.extend(warnings);
                    function.insert("output_schema".to_owned(), schema);
                    out.provider_tools
                        .output_schema_tool_names
                        .insert(name.as_str().to_owned());
                }
                let item = JsonValue::Object(function);
                match options.namespace {
                    Some(namespace) => {
                        if let Some(entry) = namespaces.iter_mut().find(|(n, _, _)| *n == namespace)
                        {
                            if entry.1 != options.namespace_description {
                                return Err(ProviderError::unsupported(format!(
                                    "conflicting descriptions for OpenAI tool namespace {namespace}"
                                )));
                            }
                            if let Some(children) = converted[entry.2]["tools"].as_array_mut() {
                                children.push(item);
                            }
                        } else {
                            let mut group =
                                json!({"type":"namespace","name":namespace,"tools":[item]});
                            if let Some(description) = &options.namespace_description {
                                group["description"] = json!(description);
                            }
                            namespaces.push((
                                namespace,
                                options.namespace_description,
                                converted.len(),
                            ));
                            converted.push(group);
                        }
                    }
                    None => converted.push(item),
                }
            }
            ToolDefinition::Provider { id, name, args } => {
                let Some(provider_type) = provider_tool_type(id) else {
                    continue;
                };
                match provider_type {
                    "tool_search" => out.provider_tools.tool_search = true,
                    "programmatic_tool_calling" => out.provider_tools.programmatic = true,
                    "apply_patch" => out.provider_tools.apply_patch = true,
                    "local_shell" => out.provider_tools.local_shell = true,
                    "shell" => out.provider_tools.shell = true,
                    "computer" => out.provider_tools.computer = true,
                    "custom" => {
                        out.provider_tools
                            .custom_tool_names
                            .insert(name.as_str().to_owned());
                    }
                    "web_search" | "web_search_preview" => {
                        out.has_web_search = true;
                        out.web_search_tool_name = Some(name.as_str().to_owned());
                    }
                    "code_interpreter" => out.has_code_interpreter = true,
                    _ => {}
                }
                let args = crate::tools::schemas::arguments(id, args)?;
                converted.push(provider_tool_item(provider_type, name.as_str(), &args)?);
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => out
                .warnings
                .push(Warning::unsupported("tool definition type")),
        }
    }
    out.tools = Some(converted);
    out.tool_choice = tool_choice.and_then(|choice| convert_tool_choice(choice, tools, mapping));
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
                        "domainSecrets" => "domain_secrets",
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

fn provider_tool_item(
    provider_type: &str,
    name: &str,
    args: &JsonObject,
) -> Result<JsonValue, ProviderError> {
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
            item.insert("name".to_owned(), JsonValue::from(name));
            for key in ["description", "format", "async"] {
                if let Some(value) = args.get(key) {
                    item.insert(key.to_owned(), value.clone());
                }
            }
        }
        "shell" => {
            if let Some(environment) = args.get("environment") {
                item.insert(
                    "environment".into(),
                    super::tool_options::shell_environment(environment)?,
                );
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
    if provider_type == "mcp" {
        item.entry("require_approval")
            .or_insert_with(|| JsonValue::from("never"));
    }
    Ok(JsonValue::Object(item))
}

fn convert_tool_choice(
    choice: &ToolChoice,
    tools: &[ToolDefinition],
    mapping: &ToolNameMapping,
) -> Option<JsonValue> {
    match choice {
        ToolChoice::Auto => Some(JsonValue::from("auto")),
        ToolChoice::None => Some(JsonValue::from("none")),
        ToolChoice::Required => Some(JsonValue::from("required")),
        ToolChoice::Tool { tool_name } => {
            let name = mapping.to_provider_tool_name(tool_name.as_str());
            if matches!(
                name,
                "code_interpreter"
                    | "file_search"
                    | "image_generation"
                    | "web_search_preview"
                    | "web_search"
                    | "mcp"
                    | "apply_patch"
                    | "computer"
                    | "programmatic_tool_calling"
            ) {
                Some(json!({"type":name}))
            } else if tools.iter().any(|tool| {
                matches!(tool,
                    ToolDefinition::Provider {id,name: tool_name,..}
                    if id == "openai.custom" && tool_name.as_str() == name
                )
            }) {
                Some(json!({"type":"custom","name":name}))
            } else {
                Some(json!({"type":"function","name":name}))
            }
        }
        #[allow(unreachable_patterns, reason = "ToolChoice is non-exhaustive")]
        _ => None,
    }
}
