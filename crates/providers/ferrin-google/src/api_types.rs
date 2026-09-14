//! Wire types of the `generateContent` responses (requests are built as JSON
//! objects in [`crate::request`]).

use ferrin_spec::JsonValue;
use serde::Deserialize;

use crate::json_accumulator::PartialArg;

/// A `generateContent` response or stream chunk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    /// Candidates (the first one is used).
    #[serde(default)]
    pub candidates: Option<Vec<Candidate>>,
    /// Token usage.
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
    /// Prompt feedback (`{blockReason, blockReasonMessage, safetyRatings}`).
    #[serde(default)]
    pub prompt_feedback: Option<JsonValue>,
    /// Response id.
    #[serde(default)]
    pub response_id: Option<String>,
    /// Model version that produced the response.
    #[serde(default)]
    pub model_version: Option<String>,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub create_time: Option<String>,
}

impl GenerateContentResponse {
    /// `promptFeedback.blockReason`.
    #[must_use]
    pub fn block_reason(&self) -> Option<&str> {
        self.prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.get("blockReason"))
            .and_then(JsonValue::as_str)
    }

    /// The first candidate.
    #[must_use]
    pub fn candidate(&self) -> Option<&Candidate> {
        self.candidates
            .as_ref()
            .and_then(|candidates| candidates.first())
    }
}

/// A response candidate.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Generated content.
    #[serde(default)]
    pub content: Option<CandidateContent>,
    /// Finish reason (`STOP`, `MAX_TOKENS`, `SAFETY`, ...).
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// Finish message.
    #[serde(default)]
    pub finish_message: Option<String>,
    /// Safety ratings.
    #[serde(default)]
    pub safety_ratings: Option<JsonValue>,
    /// Grounding metadata (search and file search results).
    #[serde(default)]
    pub grounding_metadata: Option<JsonValue>,
    /// URL context metadata.
    #[serde(default)]
    pub url_context_metadata: Option<JsonValue>,
}

impl Candidate {
    /// Parts of the candidate content.
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        self.content
            .as_ref()
            .and_then(|content| content.parts.as_deref())
            .unwrap_or_default()
    }

    /// Grounding chunks of the grounding metadata.
    #[must_use]
    pub fn grounding_chunks(&self) -> Vec<GroundingChunk> {
        self.grounding_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("groundingChunks"))
            .and_then(JsonValue::as_array)
            .map(|chunks| {
                chunks
                    .iter()
                    .filter_map(|chunk| serde_json::from_value(chunk.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Content of a candidate.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateContent {
    /// Parts.
    #[serde(default)]
    pub parts: Option<Vec<Part>>,
    /// Role (`model`).
    #[serde(default)]
    pub role: Option<String>,
}

/// A content part.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// Text.
    #[serde(default)]
    pub text: Option<String>,
    /// Whether the part is a thought summary.
    #[serde(default)]
    pub thought: Option<bool>,
    /// Thought signature to send back with the part.
    #[serde(default)]
    pub thought_signature: Option<String>,
    /// Function call.
    #[serde(default)]
    pub function_call: Option<FunctionCall>,
    /// Inline binary data.
    #[serde(default)]
    pub inline_data: Option<InlineData>,
    /// Code produced by the code execution tool.
    #[serde(default)]
    pub executable_code: Option<ExecutableCode>,
    /// Result of the code execution tool.
    #[serde(default)]
    pub code_execution_result: Option<CodeExecutionResult>,
    /// Server-side tool call.
    #[serde(default)]
    pub tool_call: Option<ServerToolCall>,
    /// Server-side tool response (opaque).
    #[serde(default)]
    pub tool_response: Option<JsonValue>,
}

/// A function call part.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    /// Call id.
    #[serde(default)]
    pub id: Option<String>,
    /// Function name.
    #[serde(default)]
    pub name: Option<String>,
    /// Complete arguments.
    #[serde(default)]
    pub args: Option<JsonValue>,
    /// Streamed argument fragments.
    #[serde(default)]
    pub partial_args: Option<Vec<PartialArg>>,
    /// Whether more fragments of this call follow.
    #[serde(default)]
    pub will_continue: Option<bool>,
}

impl FunctionCall {
    /// A fragment of a streamed call (partial arguments, or a name announced
    /// with `willContinue`).
    #[must_use]
    pub fn is_streaming_fragment(&self) -> bool {
        self.partial_args.is_some() || (self.name.is_some() && self.will_continue == Some(true))
    }

    /// The `{}` fragment that terminates a streamed call.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.name.is_none()
            && self.args.is_none()
            && self.partial_args.is_none()
            && self.will_continue.is_none()
    }

    /// Whether this fragment completes the streamed call.
    #[must_use]
    pub fn completes_stream(&self) -> bool {
        self.will_continue != Some(true)
            && self
                .partial_args
                .as_ref()
                .is_none_or(|args| args.iter().all(|arg| arg.will_continue != Some(true)))
    }
}

/// Inline binary data (`{mimeType, data}` with base64 data).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    /// Media type.
    #[serde(default)]
    pub mime_type: String,
    /// Base64 data.
    #[serde(default)]
    pub data: String,
}

/// Code generated by the code execution tool.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableCode {
    /// Language (`PYTHON`).
    #[serde(default)]
    pub language: Option<String>,
    /// Code.
    #[serde(default)]
    pub code: Option<String>,
}

/// Result of the code execution tool.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeExecutionResult {
    /// Outcome (`OUTCOME_OK`, ...).
    #[serde(default)]
    pub outcome: Option<String>,
    /// Output.
    #[serde(default)]
    pub output: Option<String>,
}

/// A server-side tool call (`{toolType, args, id}`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerToolCall {
    /// Tool type.
    #[serde(default)]
    pub tool_type: Option<String>,
    /// Arguments.
    #[serde(default)]
    pub args: Option<JsonValue>,
    /// Call id.
    #[serde(default)]
    pub id: Option<String>,
}

/// Token usage (`usageMetadata`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Prompt tokens.
    #[serde(default)]
    pub prompt_token_count: Option<u64>,
    /// Candidate (text output) tokens.
    #[serde(default)]
    pub candidates_token_count: Option<u64>,
    /// Cached prompt tokens.
    #[serde(default)]
    pub cached_content_token_count: Option<u64>,
    /// Thinking tokens.
    #[serde(default)]
    pub thoughts_token_count: Option<u64>,
    /// Total tokens.
    #[serde(default)]
    pub total_token_count: Option<u64>,
    /// Service tier that served the request.
    #[serde(default)]
    pub service_tier: Option<String>,
}

/// A grounding chunk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingChunk {
    /// Web result.
    #[serde(default)]
    pub web: Option<WebChunk>,
    /// Image result.
    #[serde(default)]
    pub image: Option<ImageChunk>,
    /// Retrieved context (file search, RAG).
    #[serde(default)]
    pub retrieved_context: Option<RetrievedContextChunk>,
    /// Maps result.
    #[serde(default)]
    pub maps: Option<WebChunk>,
}

/// A web (or maps) grounding chunk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChunk {
    /// URI.
    #[serde(default)]
    pub uri: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
}

/// An image grounding chunk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageChunk {
    /// Page the image was found on.
    #[serde(default)]
    pub source_uri: Option<String>,
    /// Image URI.
    #[serde(default)]
    pub image_uri: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
}

/// A retrieved-context grounding chunk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievedContextChunk {
    /// Document URI.
    #[serde(default)]
    pub uri: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Retrieved text.
    #[serde(default)]
    pub text: Option<String>,
    /// File search store name.
    #[serde(default)]
    pub file_search_store: Option<String>,
}

/// `google.rpc.Status` as embedded in operations and batch result lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RpcStatus {
    /// Canonical error code.
    #[serde(default)]
    pub code: Option<i64>,
    /// Message.
    #[serde(default)]
    pub message: Option<String>,
    /// Status name (`CANCELLED`, `INVALID_ARGUMENT`, ...).
    #[serde(default)]
    pub status: Option<String>,
}

/// Deserializes a count that the API renders either as a number or as a
/// decimal string (`"42"`); anything else becomes `None`.
///
/// # Errors
///
/// Returns the deserializer's error when the value is not valid JSON.
pub fn deserialize_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let value = Option::<JsonValue>::deserialize(deserializer)?;
    Ok(match value {
        Some(JsonValue::String(text)) => text.parse().ok(),
        Some(JsonValue::Number(number)) => number.as_u64(),
        _ => None,
    })
}
