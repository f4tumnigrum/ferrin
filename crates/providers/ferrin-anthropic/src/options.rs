//! Provider options read from `provider_options["anthropic"]` (and the
//! configured provider name when it differs).

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::InvalidArgumentError;
use serde::Deserialize;
use serde::Serialize;

use crate::config::AnthropicConfig;
use crate::config::CANONICAL_OPTIONS_KEY;

/// Cache control breakpoint (`{type: "ephemeral", ttl?: "5m" | "1h"}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheControl {
    /// Always `ephemeral`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Time to live (`5m`, `1h`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// Prefix mismatch behaviour of block binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockBinding {
    /// `error` or `drop_block`.
    #[serde(alias = "prefix_mismatch_behavior")]
    pub prefix_mismatch_behavior: String,
}

/// Extended thinking configuration (`{type: adaptive | enabled | disabled,
/// budgetTokens?, display?, blockBinding?}`; `type` may be omitted when only
/// `blockBinding` is set).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thinking {
    /// `adaptive`, `enabled` or `disabled`.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Budget in tokens (`enabled`); defaults to 1024 with a warning.
    #[serde(default)]
    pub budget_tokens: Option<u32>,
    /// `omitted`, `summarized` or `updates` (`adaptive`).
    #[serde(default)]
    pub display: Option<String>,
    /// Block binding controls.
    #[serde(default)]
    pub block_binding: Option<BlockBinding>,
}

/// Request metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// External user id.
    #[serde(default)]
    pub user_id: Option<String>,
}

/// MCP server tool configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolConfiguration {
    /// Whether the server's tools are enabled.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Allowed tool names.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}

/// Remote MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    /// Always `url`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Server name.
    pub name: String,
    /// Server URL.
    pub url: String,
    /// Bearer token.
    #[serde(default)]
    pub authorization_token: Option<String>,
    /// Tool configuration.
    #[serde(default)]
    pub tool_configuration: Option<McpToolConfiguration>,
}

/// A skill loaded into the container.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContainerSkill {
    /// A skill published by Anthropic.
    Anthropic {
        /// Skill id.
        #[serde(rename = "skillId")]
        skill_id: String,
        /// Version.
        #[serde(default)]
        version: Option<String>,
    },
    /// A skill uploaded through the Skills API.
    Custom {
        /// Provider reference of the uploaded skill.
        #[serde(rename = "providerReference")]
        provider_reference: ProviderReference,
        /// Version.
        #[serde(default)]
        version: Option<String>,
    },
}

/// Container configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    /// Existing container id.
    #[serde(default)]
    pub id: Option<String>,
    /// Skills to load.
    #[serde(default)]
    pub skills: Option<Vec<ContainerSkill>>,
}

/// Task budget.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBudget {
    /// Always `tokens`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Total budget (at least 20000).
    pub total: u64,
    /// Remaining budget.
    #[serde(default)]
    pub remaining: Option<u64>,
}

/// Server-side fallback configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Fallbacks {
    /// `"default"`.
    Default(String),
    /// Explicit fallback models (wire-format objects passed through).
    Models(Vec<JsonObject>),
}

/// Context management edit.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum ContextEdit {
    /// Clears old tool uses.
    #[serde(rename = "clear_tool_uses_20250919")]
    ClearToolUses {
        /// Trigger (`{type: input_tokens | tool_uses, value}`).
        #[serde(default)]
        trigger: Option<JsonValue>,
        /// Tool uses to keep.
        #[serde(default)]
        keep: Option<JsonValue>,
        /// Minimum tokens to clear.
        #[serde(default, rename = "clearAtLeast")]
        clear_at_least: Option<JsonValue>,
        /// Whether tool inputs are cleared as well.
        #[serde(default, rename = "clearToolInputs")]
        clear_tool_inputs: Option<bool>,
        /// Tools excluded from clearing.
        #[serde(default, rename = "excludeTools")]
        exclude_tools: Option<Vec<String>>,
    },
    /// Clears old thinking blocks.
    #[serde(rename = "clear_thinking_20251015")]
    ClearThinking {
        /// `"all"` or `{type: thinking_turns, value}`.
        #[serde(default)]
        keep: Option<JsonValue>,
    },
    /// Compacts the context.
    #[serde(rename = "compact_20260112")]
    Compact {
        /// Trigger (`{type: input_tokens, value}`).
        #[serde(default)]
        trigger: Option<JsonValue>,
        /// Whether to pause after compaction.
        #[serde(default, rename = "pauseAfterCompaction")]
        pause_after_compaction: Option<bool>,
        /// Compaction instructions.
        #[serde(default)]
        instructions: Option<String>,
    },
}

/// Context management configuration.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ContextManagement {
    /// Edits.
    #[serde(default)]
    pub edits: Vec<ContextEdit>,
}

/// Language model options (`provider_options["anthropic"]`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnthropicLanguageModelOptions {
    /// Whether reasoning parts of the prompt are sent back (default `true`).
    #[serde(default)]
    pub send_reasoning: Option<bool>,
    /// `outputFormat`, `jsonTool` or `auto` (default).
    #[serde(default)]
    pub structured_output_mode: Option<String>,
    /// Extended thinking.
    #[serde(default)]
    pub thinking: Option<Thinking>,
    /// Disables parallel tool use.
    #[serde(default)]
    pub disable_parallel_tool_use: Option<bool>,
    /// Cache control of the last message.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,
    /// Request metadata.
    #[serde(default)]
    pub metadata: Option<Metadata>,
    /// MCP servers.
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServer>>,
    /// Container configuration.
    #[serde(default)]
    pub container: Option<Container>,
    /// Whether function tool input streams eagerly (default `true` when
    /// streaming).
    #[serde(default)]
    pub tool_streaming: Option<bool>,
    /// Effort (`low`, `medium`, `high`, `xhigh`, `max`).
    #[serde(default)]
    pub effort: Option<String>,
    /// Task budget.
    #[serde(default)]
    pub task_budget: Option<TaskBudget>,
    /// Speed (`fast`, `standard`).
    #[serde(default)]
    pub speed: Option<String>,
    /// Service tier (`auto`, `standard_only`).
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Inference geography (`us`, `global`).
    #[serde(default)]
    pub inference_geo: Option<String>,
    /// Server-side fallbacks.
    #[serde(default)]
    pub fallbacks: Option<Fallbacks>,
    /// Extra beta flags.
    #[serde(default)]
    pub anthropic_beta: Option<Vec<String>>,
    /// Context management.
    #[serde(default)]
    pub context_management: Option<ContextManagement>,
}

impl AnthropicLanguageModelOptions {
    /// Overlays `other` on `self`: every option set in `other` wins.
    fn merge(mut self, other: Self) -> Self {
        macro_rules! take {
            ($($field:ident),* $(,)?) => {
                $( if other.$field.is_some() { self.$field = other.$field; } )*
            };
        }
        take!(
            send_reasoning,
            structured_output_mode,
            thinking,
            disable_parallel_tool_use,
            cache_control,
            metadata,
            mcp_servers,
            container,
            tool_streaming,
            effort,
            task_budget,
            speed,
            service_tier,
            inference_geo,
            fallbacks,
            anthropic_beta,
            context_management,
        );
        self
    }
}

/// Parsed options plus which key supplied them.
#[derive(Debug, Clone, Default)]
pub struct ParsedOptions {
    /// Merged options (custom key overlays the canonical key).
    pub options: AnthropicLanguageModelOptions,
    /// Whether the configured (non-canonical) key supplied options.
    pub used_custom_key: bool,
}

/// Parses the options under the canonical `anthropic` key and, when the
/// configured name differs, under that name as well (the latter wins).
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] when either object does not match the
/// option schema.
pub fn parse_options(
    config: &AnthropicConfig,
    provider_options: &ProviderOptions,
) -> Result<ParsedOptions, InvalidArgumentError> {
    let canonical = parse_provider_options::<AnthropicLanguageModelOptions>(
        CANONICAL_OPTIONS_KEY,
        provider_options,
    )?
    .unwrap_or_default();
    let key = config.options_key();
    if key == CANONICAL_OPTIONS_KEY {
        return Ok(ParsedOptions {
            options: canonical,
            used_custom_key: false,
        });
    }
    match parse_provider_options::<AnthropicLanguageModelOptions>(key, provider_options)? {
        Some(custom) => Ok(ParsedOptions {
            options: canonical.merge(custom),
            used_custom_key: true,
        }),
        None => Ok(ParsedOptions {
            options: canonical,
            used_custom_key: false,
        }),
    }
}

/// Reads the raw option object of a part or message: the canonical key
/// first, then the configured name.
#[must_use]
pub fn part_options<'a>(
    config: &AnthropicConfig,
    provider_options: Option<&'a ProviderOptions>,
) -> Option<&'a JsonObject> {
    let options = provider_options?;
    options
        .get(config.options_key())
        .or_else(|| options.get(CANONICAL_OPTIONS_KEY))
}

/// File part options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePartOptions {
    /// Upload the file into the container instead of attaching it.
    #[serde(default)]
    pub container_upload: Option<bool>,
    /// Citation configuration.
    #[serde(default)]
    pub citations: Option<Citations>,
    /// Document title.
    #[serde(default)]
    pub title: Option<String>,
    /// Document context.
    #[serde(default)]
    pub context: Option<String>,
}

/// Citation configuration of a document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citations {
    /// Whether citations are enabled.
    pub enabled: bool,
}

/// Tool change announced by a mid-conversation system message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolChange {
    /// `tool_addition` or `tool_removal`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Tool name.
    pub tool_name: String,
}

/// System message options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMessageOptions {
    /// `next_user_message`.
    #[serde(default)]
    pub clear_at: Option<String>,
    /// Effort override.
    #[serde(default)]
    pub effort: Option<String>,
    /// Tool changes.
    #[serde(default)]
    pub tool_changes: Option<Vec<ToolChange>>,
}

/// Function tool options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOptions {
    /// Defer loading the tool definition.
    #[serde(default)]
    pub defer_loading: Option<bool>,
    /// Allowed callers (`direct`, `code_execution_20250825`, ...).
    #[serde(default)]
    pub allowed_callers: Option<Vec<String>>,
    /// Eager input streaming.
    #[serde(default)]
    pub eager_input_streaming: Option<bool>,
}

/// Reasoning part metadata (`signature` or `redactedData`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMetadata {
    /// Thinking block signature.
    #[serde(default)]
    pub signature: Option<String>,
    /// Redacted thinking data.
    #[serde(default)]
    pub redacted_data: Option<String>,
}

/// Deserializes `object` into `T`, ignoring unknown keys.
#[must_use]
pub fn read_options<T: serde::de::DeserializeOwned>(object: Option<&JsonObject>) -> Option<T> {
    let object = object?;
    serde_json::from_value(JsonValue::Object(object.clone())).ok()
}
