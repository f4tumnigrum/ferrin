//! Wire types of the Chat Completions API as served by compatible
//! endpoints (lenient: every field is optional).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;

/// Token usage.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ChatUsage {
    /// Prompt tokens.
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens.
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// Total tokens.
    #[serde(default)]
    pub total_tokens: Option<u64>,
    /// Prompt token details.
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    /// Completion token details.
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

/// Prompt token details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PromptTokensDetails {
    /// Cached tokens.
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

/// Completion token details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
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

/// Message content: a string or an array of typed parts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    /// Plain text.
    Text(String),
    /// Typed parts (`{type: "text", text}`, `{type: "thinking", thinking:
    /// [...]}`).
    Parts(Vec<JsonObject>),
}

/// Function part of a tool call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ChatFunction {
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// JSON arguments.
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Tool call in a message or a delta.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
    /// Provider extension (`google.thought_signature`).
    #[serde(default)]
    pub extra_content: Option<JsonValue>,
}

impl ChatToolCall {
    /// The Gemini thought signature carried in `extra_content`.
    #[must_use]
    pub fn thought_signature(&self) -> Option<String> {
        self.extra_content
            .as_ref()?
            .get("google")?
            .get("thought_signature")?
            .as_str()
            .filter(|signature| !signature.is_empty())
            .map(str::to_owned)
    }
}

/// Assistant message or delta.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ChatMessage {
    /// Content.
    #[serde(default)]
    pub content: Option<ChatContent>,
    /// Reasoning text (most endpoints).
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// Reasoning text (endpoints serving `gpt-oss`).
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Tool calls.
    #[serde(default)]
    pub tool_calls: Option<Vec<ChatToolCall>>,
}

impl ChatMessage {
    /// The reasoning text, whichever field carries it.
    #[must_use]
    pub fn reasoning_text(&self) -> Option<&str> {
        self.reasoning_content
            .as_deref()
            .or(self.reasoning.as_deref())
    }
}

/// Choice of a completion or chunk.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ChatChoice {
    /// Message (non-streaming).
    #[serde(default)]
    pub message: Option<ChatMessage>,
    /// Delta (streaming).
    #[serde(default)]
    pub delta: Option<ChatMessage>,
    /// Finish reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Completion response or stream chunk.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
                choice.message.is_some()
                    || choice.delta.as_ref().is_some_and(|delta| {
                        delta.content.is_some()
                            || delta.tool_calls.is_some()
                            || delta.reasoning_text().is_some()
                    })
            })
        })
    }
}
