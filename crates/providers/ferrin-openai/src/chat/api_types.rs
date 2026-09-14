//! Wire types of the Chat Completions API.

use std::collections::BTreeMap;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Serialize;

/// Request body.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ChatRequest {
    /// Model id.
    pub model: String,
    /// Messages.
    pub messages: Vec<JsonValue>,
    /// Token biases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    /// Return log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    /// Number of top log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    /// End-user identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Parallel tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Legacy max tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Response format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<JsonValue>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Verbosity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    /// Max completion tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    /// Store the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// Metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonObject>,
    /// Predicted output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<JsonObject>,
    /// Reasoning effort.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Service tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Prompt cache key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    /// Prompt cache options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<JsonObject>,
    /// Prompt cache retention.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    /// Safety identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    /// Tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<JsonValue>>,
    /// Tool choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<JsonValue>,
    /// Stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Stream options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<JsonObject>,
}

/// Token usage.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatUsage {
    /// Prompt tokens.
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens.
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// Prompt token details.
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    /// Completion token details.
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

/// Prompt token details.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptTokensDetails {
    /// Cached tokens.
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    /// Cache write tokens.
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
}

/// Completion token details.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionTokensDetails {
    /// Reasoning tokens.
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    /// Accepted prediction tokens.
    #[serde(default)]
    pub accepted_prediction_tokens: Option<u64>,
    /// Rejected prediction tokens.
    #[serde(default)]
    pub rejected_prediction_tokens: Option<u64>,
}

/// Function part of a tool call.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatFunction {
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// JSON arguments.
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Tool call in a message or a delta.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatToolCall {
    /// Position in the tool call list (deltas only).
    #[serde(default)]
    pub index: Option<usize>,
    /// Id.
    #[serde(default)]
    pub id: Option<String>,
    /// Function.
    #[serde(default)]
    pub function: Option<ChatFunction>,
}

/// URL citation annotation.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatAnnotation {
    /// Annotation type.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Citation.
    #[serde(default)]
    pub url_citation: Option<UrlCitation>,
}

/// URL citation payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UrlCitation {
    /// URL.
    pub url: String,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
}

/// Assistant message.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatMessage {
    /// Text content.
    #[serde(default)]
    pub content: Option<String>,
    /// Tool calls.
    #[serde(default)]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    /// Annotations.
    #[serde(default)]
    pub annotations: Option<Vec<ChatAnnotation>>,
}

/// Log probabilities of a choice.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatLogprobs {
    /// Content token log probabilities.
    #[serde(default)]
    pub content: Option<Vec<JsonValue>>,
}

/// Choice of a completion.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatChoice {
    /// Message.
    #[serde(default)]
    pub message: Option<ChatMessage>,
    /// Delta (streaming).
    #[serde(default)]
    pub delta: Option<ChatMessage>,
    /// Finish reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// Log probabilities.
    #[serde(default)]
    pub logprobs: Option<ChatLogprobs>,
}

/// Completion response or stream chunk.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatResponse {
    /// Id.
    #[serde(default)]
    pub id: Option<String>,
    /// Creation timestamp (seconds).
    #[serde(default)]
    pub created: Option<f64>,
    /// Model.
    #[serde(default)]
    pub model: Option<String>,
    /// Choices.
    #[serde(default)]
    pub choices: Option<Vec<ChatChoice>>,
    /// Usage.
    #[serde(default)]
    pub usage: Option<ChatUsage>,
    /// Error frame (streaming).
    #[serde(default)]
    pub error: Option<JsonValue>,
}

impl ChatResponse {
    /// Whether the chunk carries model output.
    #[must_use]
    pub fn has_output(&self) -> bool {
        self.choices.as_ref().is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice.delta.as_ref().is_some_and(|delta| {
                    delta.content.is_some()
                        || delta.tool_calls.is_some()
                        || delta.annotations.is_some()
                }) || choice.message.is_some()
            })
        })
    }
}
