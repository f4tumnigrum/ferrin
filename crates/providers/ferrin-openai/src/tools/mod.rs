//! Factories for OpenAI provider-defined and provider-executed tools.
//!
//! Each factory returns a [`ferrin_tool::Tool`] whose definition is a
//! `ToolDefinition::Provider` with the `openai.<tool>` id; the Responses
//! model converts the arguments (camelCase, as documented here) to the wire
//! format.

mod advanced;
pub(crate) mod schemas;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Tool;
use serde::Serialize;

fn args<T: Serialize>(value: &T) -> JsonObject {
    match serde_json::to_value(value) {
        Ok(JsonValue::Object(mut object)) => {
            object.retain(|_, value| !value.is_null());
            object
        }
        _ => JsonObject::new(),
    }
}

/// Approximate user location for web search tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLocation {
    /// Location type; always `approximate`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Country code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// City.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// Region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// IANA timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl UserLocation {
    /// Creates an approximate location.
    #[must_use]
    pub fn approximate() -> Self {
        Self {
            kind: "approximate",
            ..Self::default()
        }
    }
}

/// Domain filters of the web search tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchFilters {
    /// Only these domains are searched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// These domains are excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
}

/// Arguments of `openai.web_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchArgs {
    /// Whether the model may access the live web.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_web_access: Option<bool>,
    /// Domain filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<WebSearchFilters>,
    /// Context size (`low`, `medium`, `high`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_context_size: Option<String>,
    /// User location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<UserLocation>,
}

/// Arguments of `openai.web_search_preview`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchPreviewArgs {
    /// Context size (`low`, `medium`, `high`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_context_size: Option<String>,
    /// User location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<UserLocation>,
}

/// Ranking options of `openai.file_search`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchRanking {
    /// Ranker name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranker: Option<String>,
    /// Minimum score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
}

/// Arguments of `openai.file_search`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchArgs {
    /// Vector stores to search.
    pub vector_store_ids: Vec<String>,
    /// Maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_num_results: Option<u32>,
    /// Ranking options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<FileSearchRanking>,
    /// Attribute filters (passed through).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<JsonValue>,
}

/// Container of `openai.code_interpreter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CodeInterpreterContainer {
    /// An existing container id.
    Id(String),
    /// An automatically created container with optional files.
    Auto {
        /// Files to mount.
        #[serde(rename = "fileIds", skip_serializing_if = "Option::is_none")]
        file_ids: Option<Vec<String>>,
    },
}

/// Arguments of `openai.code_interpreter`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeInterpreterArgs {
    /// Container configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<CodeInterpreterContainer>,
}

/// Mask of `openai.image_generation`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputImageMask {
    /// Uploaded file id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    /// Image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

/// Arguments of `openai.image_generation`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageGenerationArgs {
    /// Action (`generate`, `edit`, `auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Background (`auto`, `opaque`, `transparent`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Input fidelity (`low`, `high`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_fidelity: Option<String>,
    /// Mask for edits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_image_mask: Option<InputImageMask>,
    /// Image model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Moderation (`auto`, `low`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<String>,
    /// Output compression (0 to 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_compression: Option<u32>,
    /// Output format (`png`, `jpeg`, `webp`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    /// Number of partial images streamed (0 to 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_images: Option<u32>,
    /// Quality (`low`, `medium`, `high`, `auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// Size (`1024x1024`, `1024x1536`, `1536x1024`, `auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

/// Arguments of `openai.mcp`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpArgs {
    /// Server label.
    pub server_label: String,
    /// Allowed tools (list of names or `{readOnly, toolNames}` object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<JsonValue>,
    /// Authorization header value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    /// Connector id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_id: Option<String>,
    /// Extra headers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Approval policy (`always`, `never` or `{never: {toolNames}}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_approval: Option<JsonValue>,
    /// Server description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_description: Option<String>,
    /// Server URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

/// Input format of `openai.custom`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CustomToolFormat {
    /// Free-form text.
    Text,
    /// Grammar-constrained input.
    Grammar {
        /// Grammar syntax (`regex`, `lark`).
        syntax: String,
        /// Grammar definition.
        definition: String,
    },
}

/// Arguments of `openai.custom`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomToolArgs {
    /// Legacy name hint; the registered tool name is used on the wire.
    pub name: String,
    /// Whether the model may continue without waiting for the tool result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#async: Option<bool>,
    /// Description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Input format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<CustomToolFormat>,
}

/// Arguments of `openai.shell`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellArgs {
    /// Execution environment (`{type: "local"}`, `{type: "containerAuto", ...}`
    /// or `{type: "containerReference", containerId}`), passed through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<JsonValue>,
}

/// Arguments of `openai.tool_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchArgs {
    /// Where the search runs (`server`, `client`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<String>,
    /// Description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parameters schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<JsonValue>,
}

/// Tool factories exposed by [`crate::OpenAiProvider::tools`].
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiTools;

impl OpenAiTools {
    /// Creates the factory set.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn executed(id: &str, arguments: JsonObject) -> Tool {
        let mut builder = Tool::provider_executed(id, arguments).input_schema(schemas::input(id));
        if let Some(output) = schemas::output(id) {
            builder = builder.output_schema(output);
        }
        builder.build()
    }

    fn defined(id: &str, arguments: JsonObject) -> Tool {
        let mut builder = Tool::provider_defined(id, arguments).input_schema(schemas::input(id));
        if let Some(output) = schemas::output(id) {
            builder = builder.output_schema(output);
        }
        builder.build()
    }

    /// `openai.web_search` (provider-executed).
    #[must_use]
    pub fn web_search(&self, arguments: WebSearchArgs) -> Tool {
        Self::executed("openai.web_search", args(&arguments))
    }

    /// `openai.web_search_preview` (provider-executed).
    #[must_use]
    pub fn web_search_preview(&self, arguments: WebSearchPreviewArgs) -> Tool {
        Self::executed("openai.web_search_preview", args(&arguments))
    }

    /// `openai.file_search` (provider-executed).
    #[must_use]
    pub fn file_search(&self, arguments: FileSearchArgs) -> Tool {
        Self::executed("openai.file_search", args(&arguments))
    }

    /// `openai.code_interpreter` (provider-executed).
    #[must_use]
    pub fn code_interpreter(&self, arguments: CodeInterpreterArgs) -> Tool {
        Self::executed("openai.code_interpreter", args(&arguments))
    }

    /// `openai.image_generation` (provider-executed).
    #[must_use]
    pub fn image_generation(&self, arguments: ImageGenerationArgs) -> Tool {
        Self::executed("openai.image_generation", args(&arguments))
    }

    /// `openai.mcp` (provider-executed, dynamic results).
    #[must_use]
    pub fn mcp(&self, arguments: McpArgs) -> Tool {
        Self::executed("openai.mcp", args(&arguments))
    }

    /// `openai.tool_search` (provider-executed by default).
    #[must_use]
    pub fn tool_search(&self, arguments: ToolSearchArgs) -> Tool {
        advanced::search(args(&arguments))
    }

    /// `openai.programmatic_tool_calling` (provider-executed).
    #[must_use]
    pub fn programmatic_tool_calling(&self) -> Tool {
        advanced::programmatic()
    }

    /// `openai.apply_patch` (client-executed).
    #[must_use]
    pub fn apply_patch(&self) -> Tool {
        Self::defined("openai.apply_patch", JsonObject::new())
    }

    /// `openai.local_shell` (client-executed).
    #[must_use]
    pub fn local_shell(&self) -> Tool {
        Self::defined("openai.local_shell", JsonObject::new())
    }

    /// `openai.shell` (client-executed unless a container environment is set).
    #[must_use]
    pub fn shell(&self, arguments: ShellArgs) -> Tool {
        advanced::shell(args(&arguments))
    }

    /// `openai.computer` (client-executed).
    #[must_use]
    pub fn computer(&self) -> Tool {
        Self::defined("openai.computer", JsonObject::new())
    }

    /// `openai.custom` (client-executed, free-form input).
    #[must_use]
    pub fn custom(&self, arguments: CustomToolArgs) -> Tool {
        Self::defined("openai.custom", args(&arguments))
    }
}
