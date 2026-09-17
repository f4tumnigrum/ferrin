//! Factories for Anthropic provider-defined and provider-executed tools.
//!
//! Each factory returns a [`ferrin_tool::Tool`] whose definition is a
//! `ToolDefinition::Provider` with the `anthropic.<tool>` id; the request
//! preparation converts the arguments (camelCase, as documented here) to the
//! wire format and adds the beta flags the tool needs.

mod code_execution;
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

fn defined<T: Serialize>(id: &str, value: &T) -> Tool {
    let mut builder = Tool::provider_defined(id, args(value)).input_schema(schemas::input(id));
    if let Some(output) = schemas::output(id) {
        builder = builder.output_schema(output);
    }
    builder.build()
}

fn executed<T: Serialize>(id: &str, value: &T) -> Tool {
    let mut builder = Tool::provider_executed(id, args(value)).input_schema(schemas::input(id));
    if let Some(output) = schemas::output(id) {
        builder = builder.output_schema(output);
    }
    builder.build()
}

/// Arguments of the computer use tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerArgs {
    /// Screen width in pixels.
    pub display_width_px: u32,
    /// Screen height in pixels.
    pub display_height_px: u32,
    /// X11 display number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_number: Option<u32>,
    /// Whether the zoom action is enabled (`computer_20251124` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_zoom: Option<bool>,
}

/// Arguments of `anthropic.text_editor_20250728`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEditorArgs {
    /// Maximum characters returned by `view`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<u32>,
}

/// Approximate user location of the web search tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLocation {
    /// Always `approximate`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// City.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// Region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Country code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
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

/// Arguments of the web search tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchArgs {
    /// Maximum number of searches per request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    /// Only these domains are searched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// These domains are excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    /// User location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<UserLocation>,
}

/// Citation configuration of the web fetch tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CitationsArg {
    /// Whether citations are enabled.
    pub enabled: bool,
}

/// Arguments of the web fetch tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebFetchArgs {
    /// Maximum number of fetches per request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    /// Only these domains may be fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// These domains are excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    /// Citation configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<CitationsArg>,
    /// Maximum tokens of fetched content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_content_tokens: Option<u32>,
}

/// Prompt caching of the advisor tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AdvisorCaching {
    /// Always `ephemeral`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Time to live (`5m`, `1h`).
    pub ttl: String,
}

impl AdvisorCaching {
    /// Creates an ephemeral cache configuration.
    #[must_use]
    pub fn ephemeral(ttl: impl Into<String>) -> Self {
        Self {
            kind: "ephemeral",
            ttl: ttl.into(),
        }
    }
}

/// Arguments of `anthropic.advisor_20260301`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvisorArgs {
    /// Advisor model id.
    pub model: String,
    /// Maximum number of advisor calls per request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    /// Maximum advisor output tokens (at least 1024).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Prompt caching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caching: Option<AdvisorCaching>,
}

#[derive(Serialize)]
struct NoArgs {}

/// Tool factories, reachable through `AnthropicProvider::tools()`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnthropicTools;

impl AnthropicTools {
    /// Creates the factories.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// `anthropic.bash_20241022` (client executed).
    #[must_use]
    pub fn bash_20241022(&self) -> Tool {
        defined("anthropic.bash_20241022", &NoArgs {})
    }

    /// `anthropic.bash_20250124` (client executed).
    #[must_use]
    pub fn bash_20250124(&self) -> Tool {
        defined("anthropic.bash_20250124", &NoArgs {})
    }

    /// `anthropic.computer_20241022` (client executed).
    #[must_use]
    pub fn computer_20241022(&self, args: ComputerArgs) -> Tool {
        defined("anthropic.computer_20241022", &args)
    }

    /// `anthropic.computer_20250124` (client executed).
    #[must_use]
    pub fn computer_20250124(&self, args: ComputerArgs) -> Tool {
        defined("anthropic.computer_20250124", &args)
    }

    /// `anthropic.computer_20251124` (client executed).
    #[must_use]
    pub fn computer_20251124(&self, args: ComputerArgs) -> Tool {
        defined("anthropic.computer_20251124", &args)
    }

    /// `anthropic.text_editor_20241022` (client executed).
    #[must_use]
    pub fn text_editor_20241022(&self) -> Tool {
        defined("anthropic.text_editor_20241022", &NoArgs {})
    }

    /// `anthropic.text_editor_20250124` (client executed).
    #[must_use]
    pub fn text_editor_20250124(&self) -> Tool {
        defined("anthropic.text_editor_20250124", &NoArgs {})
    }

    /// `anthropic.text_editor_20250429` (client executed).
    #[must_use]
    pub fn text_editor_20250429(&self) -> Tool {
        defined("anthropic.text_editor_20250429", &NoArgs {})
    }

    /// `anthropic.text_editor_20250728` (client executed).
    #[must_use]
    pub fn text_editor_20250728(&self, args: TextEditorArgs) -> Tool {
        defined("anthropic.text_editor_20250728", &args)
    }

    /// `anthropic.memory_20250818` (client executed).
    #[must_use]
    pub fn memory_20250818(&self) -> Tool {
        defined("anthropic.memory_20250818", &NoArgs {})
    }

    /// `anthropic.web_search_20250305` (provider executed).
    #[must_use]
    pub fn web_search_20250305(&self, args: WebSearchArgs) -> Tool {
        executed("anthropic.web_search_20250305", &args)
    }

    /// `anthropic.web_search_20260209` (provider executed).
    #[must_use]
    pub fn web_search_20260209(&self, args: WebSearchArgs) -> Tool {
        executed("anthropic.web_search_20260209", &args)
    }

    /// `anthropic.web_fetch_20250910` (provider executed).
    #[must_use]
    pub fn web_fetch_20250910(&self, args: WebFetchArgs) -> Tool {
        executed("anthropic.web_fetch_20250910", &args)
    }

    /// `anthropic.web_fetch_20260209` (provider executed).
    #[must_use]
    pub fn web_fetch_20260209(&self, args: WebFetchArgs) -> Tool {
        executed("anthropic.web_fetch_20260209", &args)
    }

    /// `anthropic.code_execution_20250522` (provider executed).
    #[must_use]
    pub fn code_execution_20250522(&self) -> Tool {
        executed("anthropic.code_execution_20250522", &NoArgs {})
    }

    /// `anthropic.code_execution_20250825` (provider executed).
    #[must_use]
    pub fn code_execution_20250825(&self) -> Tool {
        code_execution::tool("code_execution_20250825")
    }

    /// `anthropic.code_execution_20260120` (provider executed).
    #[must_use]
    pub fn code_execution_20260120(&self) -> Tool {
        code_execution::tool("code_execution_20260120")
    }

    /// `anthropic.tool_search_regex_20251119` (provider executed).
    #[must_use]
    pub fn tool_search_regex_20251119(&self) -> Tool {
        executed("anthropic.tool_search_regex_20251119", &NoArgs {})
    }

    /// `anthropic.tool_search_bm25_20251119` (provider executed).
    #[must_use]
    pub fn tool_search_bm25_20251119(&self) -> Tool {
        executed("anthropic.tool_search_bm25_20251119", &NoArgs {})
    }

    /// `anthropic.advisor_20260301` (provider executed).
    #[must_use]
    pub fn advisor_20260301(&self, args: AdvisorArgs) -> Tool {
        executed("anthropic.advisor_20260301", &args)
    }
}
