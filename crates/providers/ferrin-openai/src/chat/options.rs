//! Provider options of the Chat Completions API.

use std::collections::BTreeMap;

use ferrin_spec::JsonObject;
use serde::Deserialize;

use crate::capabilities::SystemMessageMode;
use crate::responses::options::LogprobsOption;

/// Call-level provider options (`provider_options["openai"]`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatProviderOptions {
    /// Token id → bias.
    #[serde(default)]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    /// Log probabilities.
    #[serde(default)]
    pub logprobs: Option<LogprobsOption>,
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
    /// Whether tools may be called in parallel.
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    /// Explicit `max_completion_tokens`.
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
    /// Store the completion.
    #[serde(default)]
    pub store: Option<bool>,
    /// Request metadata.
    #[serde(default)]
    pub metadata: Option<JsonObject>,
    /// Predicted output.
    #[serde(default)]
    pub prediction: Option<JsonObject>,
    /// Reasoning effort.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Service tier.
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Prompt cache key.
    #[serde(default)]
    pub prompt_cache_key: Option<String>,
    /// Prompt cache options.
    #[serde(default)]
    pub prompt_cache_options: Option<JsonObject>,
    /// Prompt cache retention.
    #[serde(default)]
    pub prompt_cache_retention: Option<String>,
    /// Safety identifier.
    #[serde(default)]
    pub safety_identifier: Option<String>,
    /// Output verbosity.
    #[serde(default)]
    pub text_verbosity: Option<String>,
    /// Strict JSON schemas (default true).
    #[serde(default)]
    pub strict_json_schema: Option<bool>,
    /// System message mode.
    #[serde(default)]
    pub system_message_mode: Option<SystemMessageMode>,
    /// Treat the model as a reasoning model.
    #[serde(default)]
    pub force_reasoning: Option<bool>,
}
