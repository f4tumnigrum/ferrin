//! Provider options of the Responses API (`provider_options["openai"]`).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;

use crate::capabilities::SystemMessageMode;

/// `logprobs`: `true` for the default count or an explicit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum LogprobsOption {
    /// Enable with the default number of alternatives.
    Enabled(bool),
    /// Enable with `n` alternatives per token.
    Count(u32),
}

/// Maximum `top_logprobs` accepted by the API.
pub const TOP_LOGPROBS_MAX: u32 = 20;

impl LogprobsOption {
    /// Number of alternatives requested, `None` when disabled.
    #[must_use]
    pub fn top_logprobs(self) -> Option<u32> {
        match self {
            Self::Enabled(true) => Some(TOP_LOGPROBS_MAX),
            Self::Enabled(false) => None,
            Self::Count(count) => Some(count),
        }
    }
}

/// Prompt cache options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCacheOptions {
    /// Cache retention (`in_memory`, `24h`).
    #[serde(default)]
    pub retention: Option<String>,
}

/// `context_management` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextManagementOption {
    /// Management strategy (`compaction`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Token threshold that triggers compaction.
    #[serde(default)]
    pub compact_threshold: Option<u64>,
}

/// Call-level provider options.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponsesProviderOptions {
    /// Conversation id (mutually exclusive with `previous_response_id`).
    #[serde(default)]
    pub conversation: Option<String>,
    /// `include` values added to the request.
    #[serde(default)]
    pub include: Option<Vec<String>>,
    /// Whether to include web search sources (default true).
    #[serde(default)]
    pub include_web_search_sources: Option<bool>,
    /// Instructions (system prompt) sent separately from the input.
    #[serde(default)]
    pub instructions: Option<String>,
    /// Log probabilities.
    #[serde(default)]
    pub logprobs: Option<LogprobsOption>,
    /// Maximum number of tool calls.
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    /// Request metadata.
    #[serde(default)]
    pub metadata: Option<JsonObject>,
    /// Whether tools may be called in parallel.
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    /// Id of the previous response to continue.
    #[serde(default)]
    pub previous_response_id: Option<String>,
    /// Prompt cache key.
    #[serde(default)]
    pub prompt_cache_key: Option<String>,
    /// Prompt cache options.
    #[serde(default)]
    pub prompt_cache_options: Option<PromptCacheOptions>,
    /// Prompt cache retention (top-level field).
    #[serde(default)]
    pub prompt_cache_retention: Option<String>,
    /// Reasoning effort (`minimal`, `low`, `medium`, `high`, `xhigh`, `max`).
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Reasoning effort applied with a `configuration_update` item.
    #[serde(default)]
    pub reasoning_effort_update: Option<String>,
    /// Reasoning summary mode (`auto`, `concise`, `detailed`).
    #[serde(default)]
    pub reasoning_summary: Option<String>,
    /// Reasoning mode.
    #[serde(default)]
    pub reasoning_mode: Option<String>,
    /// Reasoning context.
    #[serde(default)]
    pub reasoning_context: Option<String>,
    /// Safety identifier.
    #[serde(default)]
    pub safety_identifier: Option<String>,
    /// Service tier (`auto`, `default`, `flex`, `priority`, `fast`).
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Whether the response is stored (default true).
    #[serde(default)]
    pub store: Option<bool>,
    /// Whether JSON schemas are strict (default true).
    #[serde(default)]
    pub strict_json_schema: Option<bool>,
    /// How system messages are sent.
    #[serde(default)]
    pub system_message_mode: Option<SystemMessageMode>,
    /// Output verbosity (`low`, `medium`, `high`).
    #[serde(default)]
    pub text_verbosity: Option<String>,
    /// Truncation strategy (`auto`, `disabled`).
    #[serde(default)]
    pub truncation: Option<String>,
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
    /// Treat the model as a reasoning model regardless of its id.
    #[serde(default)]
    pub force_reasoning: Option<bool>,
    /// Context management configuration.
    #[serde(default)]
    pub context_management: Option<Vec<ContextManagementOption>>,
    /// Append a `compaction_trigger` item to the input.
    #[serde(default)]
    pub compaction_trigger: Option<bool>,
    /// Send unsupported file media types unchanged instead of failing.
    #[serde(default)]
    pub pass_through_unsupported_files: Option<bool>,
}

/// Part-level options (`provider_options["openai"]` on prompt parts).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartOptions {
    /// Caller identity of a programmatic function call.
    #[serde(default)]
    pub caller: Option<JsonValue>,
    /// Whether a function is asynchronous.
    #[serde(default)]
    pub r#async: Option<bool>,
    /// Function namespace.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Item id of the part in the stored conversation.
    #[serde(default)]
    pub item_id: Option<String>,
    /// Encrypted reasoning content.
    #[serde(default)]
    pub reasoning_encrypted_content: Option<String>,
    /// Output phase (`commentary`, `final_answer`).
    #[serde(default)]
    pub phase: Option<String>,
    /// Image detail (`low`, `high`, `auto`).
    #[serde(default)]
    pub image_detail: Option<String>,
    /// Encrypted compaction content.
    #[serde(default)]
    pub encrypted_content: Option<String>,
}

/// Tool-level options (`provider_options["openai"]` on function tools).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionToolOptions {
    /// Whether the tool is asynchronous.
    #[serde(default)]
    pub r#async: Option<bool>,
    /// Defer loading of the tool definition.
    #[serde(default)]
    pub defer_loading: Option<bool>,
    /// Callers allowed to invoke the tool.
    #[serde(default)]
    pub allowed_callers: Option<Vec<String>>,
    /// Output schema of the tool.
    #[serde(default)]
    pub output_schema: Option<JsonValue>,
    /// Namespace grouping for the tool.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Description of the namespace.
    #[serde(default)]
    pub namespace_description: Option<String>,
}

/// Options for the allowed-tools tool choice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedToolsOptions {
    /// Selection mode (`auto`, `required`).
    #[serde(default)]
    pub mode: Option<String>,
    /// Names of the allowed tools.
    #[serde(default)]
    pub tools: Vec<String>,
}
