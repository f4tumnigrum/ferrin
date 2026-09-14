//! Request and response types of the Responses API.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Serialize;

/// Request body of `POST /responses`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ResponsesRequest {
    /// Model id.
    pub model: String,
    /// Input items.
    pub input: Vec<JsonValue>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Output token limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Text output configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<JsonObject>,
    /// Conversation id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    /// Tool call limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
    /// Request metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonObject>,
    /// Parallel tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Previous response id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Store the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// End-user id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Service tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Include list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
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
    /// Top logprobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    /// Truncation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    /// Context management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Vec<JsonObject>>,
    /// Reasoning configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<JsonObject>,
    /// Tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<JsonValue>>,
    /// Tool choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<JsonValue>,
    /// Stream flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Body of a completed response.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesResponse {
    /// Response id.
    #[serde(default)]
    pub id: Option<String>,
    /// Creation time (epoch seconds).
    #[serde(default)]
    pub created_at: Option<f64>,
    /// Model id.
    #[serde(default)]
    pub model: Option<String>,
    /// Error, when the response failed.
    #[serde(default)]
    pub error: Option<ResponseError>,
    /// Output items.
    #[serde(default)]
    pub output: Option<Vec<OutputItem>>,
    /// Why the response is incomplete.
    #[serde(default)]
    pub incomplete_details: Option<IncompleteDetails>,
    /// Service tier used.
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Reasoning configuration echoed back.
    #[serde(default)]
    pub reasoning: Option<ResponseReasoning>,
    /// Token usage.
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

/// Error object of a failed response.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseError {
    /// Error code.
    #[serde(default)]
    pub code: Option<JsonValue>,
    /// Error message.
    #[serde(default)]
    pub message: Option<String>,
}

/// `incomplete_details`.
#[derive(Debug, Clone, Deserialize)]
pub struct IncompleteDetails {
    /// Reason (`max_output_tokens`, `content_filter`).
    #[serde(default)]
    pub reason: Option<String>,
}

/// `reasoning` echoed in the response.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseReasoning {
    /// Reasoning context.
    #[serde(default)]
    pub context: Option<JsonValue>,
}

/// Token usage of a response.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResponsesUsage {
    /// Input tokens.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Input token details.
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Output token details.
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

/// `input_tokens_details`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InputTokensDetails {
    /// Cached input tokens.
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    /// Cache write tokens.
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
}

/// `output_tokens_details`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OutputTokensDetails {
    /// Reasoning tokens.
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

/// An output item; the JSON is kept for fields specific to each type.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputItem {
    /// Item type (`message`, `reasoning`, `function_call`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    /// Item id.
    #[serde(default)]
    pub id: Option<String>,
    /// Status (`completed`, `in_progress`, ...).
    #[serde(default)]
    pub status: Option<String>,
    /// Every other field.
    #[serde(flatten)]
    pub rest: JsonObject,
}

impl OutputItem {
    /// Returns a field of the item.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.rest.get(key)
    }

    /// Returns a string field of the item.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.rest.get(key).and_then(JsonValue::as_str)
    }

    /// Returns the item id or an empty string.
    #[must_use]
    pub fn id_str(&self) -> &str {
        self.id.as_deref().unwrap_or_default()
    }
}

/// A streamed event of `POST /responses` with `stream: true`.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesChunk {
    /// Event type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Every other field.
    #[serde(flatten)]
    pub rest: JsonObject,
}

impl ResponsesChunk {
    /// Returns a field of the chunk.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.rest.get(key)
    }

    /// Returns a string field of the chunk.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.rest.get(key).and_then(JsonValue::as_str)
    }

    /// Returns an unsigned integer field of the chunk.
    #[must_use]
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.rest.get(key).and_then(JsonValue::as_u64)
    }

    /// Deserializes the `item` field as an output item.
    #[must_use]
    pub fn item(&self) -> Option<OutputItem> {
        let item = self.rest.get("item")?.clone();
        serde_json::from_value(item).ok()
    }

    /// Deserializes the `response` field.
    #[must_use]
    pub fn response(&self) -> Option<ResponsesResponse> {
        let response = self.rest.get("response")?.clone();
        serde_json::from_value(response).ok()
    }
}
