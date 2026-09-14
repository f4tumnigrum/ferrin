//! Call options passed to language model adapters.

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::prompt::Prompt;
use super::tool::ToolDefinition;
use crate::json::JsonValue;
use crate::shared::Headers;
use crate::shared::ProviderOptions;
use crate::shared::ToolName;

/// Options for a single `do_generate` or `do_stream` call.
///
/// The core validates and normalizes application settings before building
/// this struct; adapters translate it into a provider request. Options the
/// provider does not support must produce a [`Warning`](crate::Warning) and be
/// ignored rather than cause an error.
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    /// The prompt in specification form.
    pub prompt: Prompt,
    /// Maximum number of output tokens.
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Nucleus sampling probability mass.
    pub top_p: Option<f64>,
    /// Top-k sampling.
    pub top_k: Option<u32>,
    /// Presence penalty.
    pub presence_penalty: Option<f64>,
    /// Frequency penalty.
    pub frequency_penalty: Option<f64>,
    /// Stop sequences.
    pub stop_sequences: Option<Vec<String>>,
    /// Random seed for deterministic sampling.
    pub seed: Option<u64>,
    /// Output format (text or JSON with an optional schema).
    pub response_format: Option<ResponseFormat>,
    /// Tools available to the model.
    pub tools: Vec<ToolDefinition>,
    /// Tool choice constraint.
    pub tool_choice: Option<ToolChoice>,
    /// Whether the stream should include `Raw` parts with provider chunks.
    pub include_raw_chunks: bool,
    /// Requested reasoning effort.
    pub reasoning: ReasoningEffort,
    /// Additional request headers.
    pub headers: Headers,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Cancellation token; when triggered the adapter aborts the request.
    pub cancellation: CancellationToken,
}

impl CallOptions {
    /// Creates call options for `prompt` with every other field at its default.
    #[must_use]
    pub fn new(prompt: Prompt) -> Self {
        Self {
            prompt,
            ..Self::default()
        }
    }

    /// Returns a serializable copy without the cancellation token.
    ///
    /// Used for fixtures, snapshots and telemetry. Sensitive header values
    /// are masked when the record is serialized.
    #[must_use]
    pub fn to_recordable(&self) -> CallOptionsRecord {
        CallOptionsRecord {
            prompt: self.prompt.clone(),
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            stop_sequences: self.stop_sequences.clone(),
            seed: self.seed,
            response_format: self.response_format.clone(),
            tools: self.tools.clone(),
            tool_choice: self.tool_choice.clone(),
            include_raw_chunks: self.include_raw_chunks,
            reasoning: self.reasoning,
            headers: self.headers.clone(),
            provider_options: self.provider_options.clone(),
        }
    }

    /// Returns the provider options stored under `provider_key`, if any.
    #[must_use]
    pub fn provider_options_for(&self, provider_key: &str) -> Option<&crate::json::JsonObject> {
        self.provider_options.get(provider_key)
    }
}

/// Serializable projection of [`CallOptions`] (everything but cancellation).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CallOptionsRecord {
    /// See [`CallOptions::prompt`].
    pub prompt: Prompt,
    /// See [`CallOptions::max_output_tokens`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// See [`CallOptions::temperature`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// See [`CallOptions::top_p`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// See [`CallOptions::top_k`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// See [`CallOptions::presence_penalty`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// See [`CallOptions::frequency_penalty`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// See [`CallOptions::stop_sequences`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// See [`CallOptions::seed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// See [`CallOptions::response_format`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// See [`CallOptions::tools`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    /// See [`CallOptions::tool_choice`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// See [`CallOptions::include_raw_chunks`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_raw_chunks: bool,
    /// See [`CallOptions::reasoning`].
    #[serde(default)]
    pub reasoning: ReasoningEffort,
    /// See [`CallOptions::headers`]; serialized with sensitive values masked.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
    /// See [`CallOptions::provider_options`].
    #[serde(default, skip_serializing_if = "ProviderOptions::is_empty")]
    pub provider_options: ProviderOptions,
}

impl From<CallOptionsRecord> for CallOptions {
    fn from(record: CallOptionsRecord) -> Self {
        Self {
            prompt: record.prompt,
            max_output_tokens: record.max_output_tokens,
            temperature: record.temperature,
            top_p: record.top_p,
            top_k: record.top_k,
            presence_penalty: record.presence_penalty,
            frequency_penalty: record.frequency_penalty,
            stop_sequences: record.stop_sequences,
            seed: record.seed,
            response_format: record.response_format,
            tools: record.tools,
            tool_choice: record.tool_choice,
            include_raw_chunks: record.include_raw_chunks,
            reasoning: record.reasoning,
            headers: record.headers,
            provider_options: record.provider_options,
            cancellation: CancellationToken::new(),
        }
    }
}

/// Requested output format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponseFormat {
    /// Free-form text.
    Text,
    /// JSON output, optionally constrained by a JSON Schema.
    Json {
        /// JSON Schema (draft-07) the output must satisfy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema: Option<JsonValue>,
        /// Schema name passed to providers that require one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Schema description passed to providers that accept one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl ResponseFormat {
    /// JSON output constrained by `schema`, without name or description.
    #[must_use]
    pub fn json(schema: JsonValue) -> Self {
        Self::Json {
            schema: Some(schema),
            name: None,
            description: None,
        }
    }

    /// JSON output without a schema.
    #[must_use]
    pub fn json_unconstrained() -> Self {
        Self::Json {
            schema: None,
            name: None,
            description: None,
        }
    }
}

/// Constraint on which tool the model may call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolChoice {
    /// The model decides whether to call a tool.
    Auto,
    /// The model must not call a tool.
    None,
    /// The model must call one of the available tools.
    Required,
    /// The model must call the named tool.
    Tool {
        /// Name of the required tool.
        tool_name: ToolName,
    },
}

impl ToolChoice {
    /// Requires the model to call the tool named `name`.
    #[must_use]
    pub fn tool(name: impl Into<ToolName>) -> Self {
        Self::Tool {
            tool_name: name.into(),
        }
    }
}

/// Requested reasoning effort.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEffort {
    /// Let the provider apply its default.
    #[default]
    ProviderDefault,
    /// Disable reasoning where the provider allows it.
    None,
    /// Minimal effort.
    Minimal,
    /// Low effort.
    Low,
    /// Medium effort.
    Medium,
    /// High effort.
    High,
    /// Extra-high effort.
    #[serde(rename = "xhigh")]
    XHigh,
}

impl ReasoningEffort {
    /// Returns the wire representation (`provider-default`, `none`, ..., `xhigh`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderDefault => "provider-default",
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
