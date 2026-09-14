//! Conversion of tool definitions and tool choice to `tools` and
//! `toolConfig`.

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use ferrin_spec::shared::Warning;
use serde_json::json;

use crate::capabilities::ModelCapabilities;
use crate::json_schema::convert_json_schema_to_openapi_schema;
use crate::json_schema::is_recursive_reference_error;

/// Provider tool ids understood by this crate.
pub mod ids {
    /// Google Search grounding.
    pub const GOOGLE_SEARCH: &str = "google.google_search";
    /// Enterprise web search (Vertex AI).
    pub const ENTERPRISE_WEB_SEARCH: &str = "google.enterprise_web_search";
    /// URL context.
    pub const URL_CONTEXT: &str = "google.url_context";
    /// Code execution.
    pub const CODE_EXECUTION: &str = "google.code_execution";
    /// File search.
    pub const FILE_SEARCH: &str = "google.file_search";
    /// Vertex AI RAG store retrieval.
    pub const VERTEX_RAG_STORE: &str = "google.vertex_rag_store";
    /// Google Maps grounding.
    pub const GOOGLE_MAPS: &str = "google.google_maps";
}

/// Wire name of the code execution tool (mapped from `google.code_execution`).
pub const CODE_EXECUTION_TOOL_NAME: &str = "code_execution";

/// `tools` and `toolConfig` of a request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreparedTools {
    /// `tools` array.
    pub tools: Option<Vec<JsonValue>>,
    /// `toolConfig` object.
    pub tool_config: Option<JsonObject>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

fn unsupported_tool(id: &str, details: &str) -> Warning {
    Warning::unsupported_with_details(format!("provider tool {id}"), details)
}

const GEMINI2_ONLY: &str = "the tool is only supported with Gemini 2 and later models";

fn provider_tool(
    id: &str,
    args: &JsonObject,
    capabilities: ModelCapabilities,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    let mut gemini2 = |wire: JsonValue| {
        if capabilities.supports_gemini2_tools {
            Some(wire)
        } else {
            warnings.push(unsupported_tool(id, GEMINI2_ONLY));
            None
        }
    };
    match id {
        ids::GOOGLE_SEARCH => gemini2(json!({"googleSearch": JsonValue::Object(args.clone())})),
        ids::ENTERPRISE_WEB_SEARCH => gemini2(json!({"enterpriseWebSearch": {}})),
        ids::URL_CONTEXT => gemini2(json!({"urlContext": {}})),
        ids::CODE_EXECUTION => gemini2(json!({"codeExecution": {}})),
        ids::GOOGLE_MAPS => Some(json!({"googleMaps": {}})),
        ids::FILE_SEARCH => {
            if capabilities.supports_file_search {
                Some(json!({"fileSearch": JsonValue::Object(args.clone())}))
            } else {
                warnings.push(unsupported_tool(
                    id,
                    "the file search tool is only supported with Gemini 2.5 and later models",
                ));
                None
            }
        }
        ids::VERTEX_RAG_STORE => {
            let mut store = JsonObject::new();
            if let Some(corpus) = args.get("ragCorpus") {
                store.insert(
                    "rag_resources".to_owned(),
                    json!({"rag_corpus": corpus.clone()}),
                );
            }
            if let Some(top_k) = args.get("topK") {
                store.insert("similarity_top_k".to_owned(), top_k.clone());
            }
            Some(json!({"retrieval": {"vertex_rag_store": store}}))
        }
        _ => {
            warnings.push(Warning::unsupported(format!("provider tool {id}")));
            None
        }
    }
}

fn function_declaration(
    name: &str,
    description: Option<&str>,
    input_schema: &JsonValue,
) -> Result<JsonValue, ProviderError> {
    let mut declaration = JsonObject::new();
    declaration.insert("name".to_owned(), JsonValue::from(name));
    declaration.insert(
        "description".to_owned(),
        JsonValue::from(description.unwrap_or_default()),
    );
    match convert_json_schema_to_openapi_schema(input_schema) {
        Ok(Some(parameters)) => {
            declaration.insert("parameters".to_owned(), parameters);
        }
        Ok(None) => {}
        Err(error) if is_recursive_reference_error(&error) => {
            declaration.insert("parametersJsonSchema".to_owned(), input_schema.clone());
        }
        Err(error) => return Err(error.into()),
    }
    Ok(JsonValue::Object(declaration))
}

fn function_calling_config(mode: &str, allowed: Option<&str>) -> JsonObject {
    let mut config = JsonObject::new();
    config.insert("mode".to_owned(), JsonValue::from(mode));
    if let Some(name) = allowed {
        config.insert("allowedFunctionNames".to_owned(), json!([name]));
    }
    config
}

/// Converts `tools` and `tool_choice`.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] when a function tool
/// schema cannot be converted (other than recursive references, which fall
/// back to `parametersJsonSchema`).
pub fn prepare_tools(
    tools: &[ToolDefinition],
    tool_choice: Option<&ToolChoice>,
    capabilities: ModelCapabilities,
    mapping: &ToolNameMapping,
    retrieval_config: Option<&JsonObject>,
) -> Result<PreparedTools, ProviderError> {
    let mut prepared = PreparedTools::default();
    if tools.is_empty() {
        prepared.tool_config = retrieval_config.map(|config| {
            let mut tool_config = JsonObject::new();
            tool_config.insert(
                "retrievalConfig".to_owned(),
                JsonValue::Object(config.clone()),
            );
            tool_config
        });
        return Ok(prepared);
    }
    let mut declarations = Vec::new();
    let mut provider_tools = Vec::new();
    let mut any_strict = false;
    for tool in tools {
        match tool {
            ToolDefinition::Function {
                name,
                description,
                input_schema,
                strict,
                ..
            } => {
                any_strict |= *strict == Some(true);
                declarations.push(function_declaration(
                    mapping.to_provider_tool_name(name.as_str()),
                    description.as_deref(),
                    input_schema,
                )?);
            }
            ToolDefinition::Provider { id, args, .. } => {
                if let Some(wire) = provider_tool(id, args, capabilities, &mut prepared.warnings) {
                    provider_tools.push(wire);
                }
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => prepared
                .warnings
                .push(Warning::unsupported("tool definition")),
        }
    }
    let has_functions = !declarations.is_empty();
    let has_provider_tools = !provider_tools.is_empty();
    let mut tool_config: Option<JsonObject> = None;
    if has_functions && has_provider_tools {
        if capabilities.uses_gemini3_features {
            let mut wire = provider_tools;
            wire.push(json!({"functionDeclarations": declarations}));
            prepared.tools = Some(wire);
            let calling = match tool_choice {
                Some(ToolChoice::None) => function_calling_config("NONE", None),
                Some(ToolChoice::Required) => function_calling_config("ANY", None),
                Some(ToolChoice::Tool { tool_name }) => function_calling_config(
                    "ANY",
                    Some(mapping.to_provider_tool_name(tool_name.as_str())),
                ),
                _ => function_calling_config("VALIDATED", None),
            };
            let mut config = JsonObject::new();
            config.insert(
                "functionCallingConfig".to_owned(),
                JsonValue::Object(calling),
            );
            config.insert(
                "includeServerSideToolInvocations".to_owned(),
                JsonValue::Bool(true),
            );
            tool_config = Some(config);
        } else {
            prepared.warnings.push(Warning::unsupported_with_details(
                "combination of function and provider-defined tools",
                "function tools and provider-defined tools cannot be mixed in one request with this model; only the provider-defined tools were sent",
            ));
            prepared.tools = Some(provider_tools);
        }
    } else if has_provider_tools {
        prepared.tools = Some(provider_tools);
    } else if has_functions {
        prepared.tools = Some(vec![json!({"functionDeclarations": declarations})]);
        let strict_mode = if any_strict { "VALIDATED" } else { "AUTO" };
        let calling = match tool_choice {
            None => any_strict.then(|| function_calling_config("VALIDATED", None)),
            Some(ToolChoice::Auto) => Some(function_calling_config(strict_mode, None)),
            Some(ToolChoice::None) => Some(function_calling_config("NONE", None)),
            Some(ToolChoice::Required) => Some(function_calling_config("ANY", None)),
            Some(ToolChoice::Tool { tool_name }) => Some(function_calling_config(
                "ANY",
                Some(mapping.to_provider_tool_name(tool_name.as_str())),
            )),
            #[allow(unreachable_patterns, reason = "ToolChoice is non-exhaustive")]
            Some(_) => None,
        };
        if let Some(calling) = calling {
            let mut config = JsonObject::new();
            config.insert(
                "functionCallingConfig".to_owned(),
                JsonValue::Object(calling),
            );
            tool_config = Some(config);
        }
    }
    if let Some(retrieval) = retrieval_config {
        tool_config.get_or_insert_with(JsonObject::new).insert(
            "retrievalConfig".to_owned(),
            JsonValue::Object(retrieval.clone()),
        );
    }
    prepared.tool_config = tool_config;
    Ok(prepared)
}
