//! Wire types of the Messages API responses and stream events.
//!
//! Request bodies are assembled as JSON values (`request.rs`,
//! `convert_prompt/`); this module only types what is read back.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;

/// Token usage of a message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AnthropicUsage {
    /// Uncached input tokens.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Output token details.
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
    /// Tokens written to the cache.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    /// Tokens read from the cache.
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    /// Per-iteration usage (compaction, fallbacks, ...).
    #[serde(default)]
    pub iterations: Option<Vec<UsageIteration>>,
}

/// Output token details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct OutputTokensDetails {
    /// Thinking tokens.
    #[serde(default)]
    pub thinking_tokens: Option<u64>,
}

/// Usage of one server-side iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct UsageIteration {
    /// `compaction`, `message`, `advisor_message` or `fallback_message`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Model that served the iteration.
    #[serde(default)]
    pub model: Option<String>,
    /// Input tokens.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Cache writes.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    /// Cache reads.
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
}

/// Stop details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StopDetails {
    /// Detail type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Category.
    #[serde(default)]
    pub category: Option<String>,
    /// Explanation.
    #[serde(default)]
    pub explanation: Option<String>,
    /// Recommended model.
    #[serde(default)]
    pub recommended_model: Option<String>,
}

/// A skill loaded in the container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ContainerSkillInfo {
    /// Skill type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Skill id.
    pub skill_id: String,
    /// Version.
    #[serde(default)]
    pub version: Option<String>,
}

/// Container information.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ContainerInfo {
    /// Container id.
    pub id: String,
    /// Expiry time.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Loaded skills.
    #[serde(default)]
    pub skills: Option<Vec<ContainerSkillInfo>>,
}

/// Applied context management edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AppliedEdit {
    /// Edit type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Cleared tool uses.
    #[serde(default)]
    pub cleared_tool_uses: Option<u64>,
    /// Cleared thinking turns.
    #[serde(default)]
    pub cleared_thinking_turns: Option<u64>,
    /// Cleared input tokens.
    #[serde(default)]
    pub cleared_input_tokens: Option<u64>,
}

/// Context management result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ResponseContextManagement {
    /// Applied edits.
    #[serde(default)]
    pub applied_edits: Vec<AppliedEdit>,
}

/// Caller of a tool use block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Caller {
    /// `direct`, `code_execution_20250825`, `code_execution_20260120`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Id of the code execution tool call that issued the call.
    #[serde(default)]
    pub tool_id: Option<String>,
}

/// A content block of a message or `content_block_start` event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// Text.
    #[serde(rename = "text")]
    Text {
        /// Text (may be empty in stream starts).
        #[serde(default)]
        text: String,
        /// Citations attached to the text.
        #[serde(default)]
        citations: Option<Vec<JsonValue>>,
    },
    /// Thinking.
    #[serde(rename = "thinking")]
    Thinking {
        /// Thinking text.
        #[serde(default)]
        thinking: String,
        /// Signature.
        #[serde(default)]
        signature: Option<String>,
    },
    /// Redacted thinking.
    #[serde(rename = "redacted_thinking")]
    RedactedThinking {
        /// Opaque data.
        data: String,
    },
    /// Client tool call.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Tool use id.
        id: String,
        /// Tool name.
        name: String,
        /// Input (may be empty in stream starts).
        #[serde(default)]
        input: Option<JsonValue>,
        /// Caller.
        #[serde(default)]
        caller: Option<Caller>,
    },
    /// Server tool call.
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        /// Tool use id.
        id: String,
        /// Tool name.
        name: String,
        /// Input.
        #[serde(default)]
        input: Option<JsonValue>,
        /// Caller.
        #[serde(default)]
        caller: Option<Caller>,
    },
    /// MCP tool call.
    #[serde(rename = "mcp_tool_use")]
    McpToolUse {
        /// Tool use id.
        id: String,
        /// Tool name.
        name: String,
        /// Input.
        #[serde(default)]
        input: Option<JsonValue>,
        /// Server name.
        server_name: String,
    },
    /// MCP tool result.
    #[serde(rename = "mcp_tool_result")]
    McpToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Error flag.
        #[serde(default)]
        is_error: bool,
        /// Result content.
        #[serde(default)]
        content: JsonValue,
    },
    /// Web fetch result.
    #[serde(rename = "web_fetch_tool_result")]
    WebFetchToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
        /// Caller.
        #[serde(default)]
        caller: Option<Caller>,
    },
    /// Web search result.
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result list or error object.
        content: JsonValue,
        /// Caller.
        #[serde(default)]
        caller: Option<Caller>,
    },
    /// Code execution result (20250522).
    #[serde(rename = "code_execution_tool_result")]
    CodeExecutionToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
    },
    /// Bash code execution result (20250825).
    #[serde(rename = "bash_code_execution_tool_result")]
    BashCodeExecutionToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
    },
    /// Text editor code execution result (20250825).
    #[serde(rename = "text_editor_code_execution_tool_result")]
    TextEditorCodeExecutionToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
    },
    /// Tool search result.
    #[serde(rename = "tool_search_tool_result")]
    ToolSearchToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
    },
    /// Advisor result.
    #[serde(rename = "advisor_tool_result")]
    AdvisorToolResult {
        /// Tool use id.
        tool_use_id: String,
        /// Result or error object.
        content: JsonValue,
    },
    /// Container upload marker.
    #[serde(rename = "container_upload")]
    ContainerUpload {
        /// File id.
        file_id: String,
    },
    /// Compaction summary.
    #[serde(rename = "compaction")]
    Compaction {
        /// Summary text.
        #[serde(default)]
        content: Option<String>,
    },
    /// Server-side fallback marker.
    #[serde(rename = "fallback")]
    Fallback {
        /// Model that served the request.
        #[serde(default)]
        model: Option<String>,
    },
    /// Any block type this crate does not know.
    #[serde(other)]
    Unknown,
}

/// A complete message.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AnthropicResponse {
    /// Message id.
    #[serde(default)]
    pub id: Option<String>,
    /// Model.
    #[serde(default)]
    pub model: Option<String>,
    /// Content blocks.
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    /// Stop reason.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Stop sequence.
    #[serde(default)]
    pub stop_sequence: Option<String>,
    /// Stop details.
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
    /// Input transformations.
    #[serde(default)]
    pub input_transformations: Option<JsonValue>,
    /// Usage.
    #[serde(default)]
    pub usage: AnthropicUsage,
    /// Container.
    #[serde(default)]
    pub container: Option<ContainerInfo>,
    /// Context management.
    #[serde(default)]
    pub context_management: Option<ResponseContextManagement>,
}

/// Message summary carried by `message_start`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MessageStart {
    /// Message id.
    #[serde(default)]
    pub id: Option<String>,
    /// Model.
    #[serde(default)]
    pub model: Option<String>,
    /// Usage so far.
    #[serde(default)]
    pub usage: AnthropicUsage,
    /// Pre-populated content (deferred tool calls).
    #[serde(default)]
    pub content: Option<Vec<ContentBlock>>,
    /// Container.
    #[serde(default)]
    pub container: Option<ContainerInfo>,
    /// Stop reason (compaction).
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Input transformations.
    #[serde(default)]
    pub input_transformations: Option<JsonValue>,
}

/// A `content_block_delta` payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    /// Partial tool input JSON.
    #[serde(rename = "input_json_delta")]
    InputJson {
        /// JSON fragment.
        #[serde(default)]
        partial_json: String,
    },
    /// Text fragment.
    #[serde(rename = "text_delta")]
    Text {
        /// Text.
        #[serde(default)]
        text: String,
    },
    /// Thinking fragment.
    #[serde(rename = "thinking_delta")]
    Thinking {
        /// Text.
        #[serde(default)]
        thinking: String,
    },
    /// Thinking signature.
    #[serde(rename = "signature_delta")]
    Signature {
        /// Signature.
        #[serde(default)]
        signature: String,
    },
    /// Compaction text fragment.
    #[serde(rename = "compaction_delta")]
    Compaction {
        /// Text.
        #[serde(default)]
        content: Option<String>,
    },
    /// Citation.
    #[serde(rename = "citations_delta")]
    Citations {
        /// Citation object.
        citation: JsonValue,
    },
    /// Unknown delta type.
    #[serde(other)]
    Unknown,
}

/// `message_delta.delta`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MessageDeltaBody {
    /// Stop reason.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Stop sequence.
    #[serde(default)]
    pub stop_sequence: Option<String>,
    /// Stop details.
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
    /// Container.
    #[serde(default)]
    pub container: Option<ContainerInfo>,
}

/// A stream event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicChunk {
    /// Message start.
    #[serde(rename = "message_start")]
    MessageStart {
        /// Message summary.
        message: MessageStart,
    },
    /// Content block start.
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        /// Block index.
        index: u64,
        /// Block.
        content_block: ContentBlock,
    },
    /// Content block delta.
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        /// Block index.
        index: u64,
        /// Delta.
        delta: Delta,
    },
    /// Content block stop.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        /// Block index.
        index: u64,
    },
    /// Error.
    #[serde(rename = "error")]
    Error {
        /// Error object (`{type, message}`).
        error: JsonObject,
    },
    /// Message delta.
    #[serde(rename = "message_delta")]
    MessageDelta {
        /// Delta.
        #[serde(default)]
        delta: MessageDeltaBody,
        /// Usage.
        #[serde(default)]
        usage: AnthropicUsage,
        /// Input transformations.
        #[serde(default)]
        input_transformations: Option<JsonValue>,
        /// Context management.
        #[serde(default)]
        context_management: Option<ResponseContextManagement>,
    },
    /// Message stop.
    #[serde(rename = "message_stop")]
    MessageStop,
    /// Keep-alive.
    #[serde(rename = "ping")]
    Ping,
    /// Unknown event type.
    #[serde(other)]
    Unknown,
}
