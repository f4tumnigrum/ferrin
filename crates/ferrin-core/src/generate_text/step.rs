//! Step results and the core content enum.

use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use ferrin_message::Message;
use ferrin_spec::ApprovalId;
use ferrin_spec::CustomKind;
use ferrin_spec::FileData;
use ferrin_spec::FinishReason;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::Source;
use ferrin_tool::ToolError;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::telemetry::ModelIdentity;

/// The result of one step: a model call plus the tool executions it caused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    /// Zero-based step index.
    pub step_number: u32,
    /// Application state used for this step (never sent to the provider).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_context: Option<JsonValue>,
    /// Shared tool context used for this step, before per-tool validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_context: Option<JsonValue>,
    /// Model that produced the step.
    pub model: ModelIdentity,
    /// Content parts in order.
    pub content: Vec<StepContent>,
    /// Why the model stopped.
    pub finish_reason: FinishReason,
    /// Token usage of the model call.
    pub usage: Usage,
    /// Warnings produced by the adapter.
    pub warnings: Vec<Warning>,
    /// Request metadata (body and messages when included).
    pub request: StepRequest,
    /// Response metadata and the messages to append to the history.
    pub response: StepResponse,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Timing and throughput.
    pub performance: StepPerformance,
}

impl StepResult {
    /// Concatenates all text parts.
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                StepContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Concatenates all reasoning parts, or `None` when there is none.
    #[must_use]
    pub fn reasoning_text(&self) -> Option<String> {
        let mut found = false;
        let text: String = self
            .content
            .iter()
            .filter_map(|part| match part {
                StepContent::Reasoning { text, .. } => {
                    found = true;
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        found.then_some(text)
    }

    /// Iterates over the tool calls of the step.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    /// Tool calls to tools of the static tool set.
    pub fn static_tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> + '_ {
        self.tool_calls().filter(|call| !call.dynamic)
    }

    /// Tool calls to dynamic tools (including invalid calls).
    pub fn dynamic_tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> + '_ {
        self.tool_calls().filter(|call| call.dynamic)
    }

    /// Iterates over the (final) tool results of the step.
    pub fn tool_results(&self) -> impl Iterator<Item = &ToolResult> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::ToolResult(result) => Some(result),
            _ => None,
        })
    }

    /// Iterates over the tool errors of the step.
    pub fn tool_errors(&self) -> impl Iterator<Item = &ToolExecutionError> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::ToolError(error) => Some(error),
            _ => None,
        })
    }

    /// Iterates over the approval requests of the step.
    pub fn tool_approval_requests(&self) -> impl Iterator<Item = &ToolApprovalRequestContent> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::ToolApprovalRequest(request) => Some(request),
            _ => None,
        })
    }

    /// Iterates over generated files.
    pub fn files(&self) -> impl Iterator<Item = &GeneratedFile> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::File(file) => Some(file),
            _ => None,
        })
    }

    /// Iterates over citation sources.
    pub fn sources(&self) -> impl Iterator<Item = &Source> + '_ {
        self.content.iter().filter_map(|part| match part {
            StepContent::Source(source) => Some(source),
            _ => None,
        })
    }

    /// The messages to append to the conversation history.
    #[must_use]
    pub fn response_messages(&self) -> Vec<Message> {
        self.response.messages.clone()
    }

    /// Deserializes the output of the first final result of `tool_name`.
    ///
    /// # Errors
    ///
    /// Returns the deserialization error; `Ok(None)` when no such result
    /// exists.
    pub fn tool_result_as<T: DeserializeOwned>(
        &self,
        tool_name: &str,
    ) -> Result<Option<T>, serde_json::Error> {
        self.tool_results()
            .find(|result| result.tool_name == tool_name)
            .map(|result| serde_json::from_value(result.output.clone()))
            .transpose()
    }
}

/// Request metadata of a step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepRequest {
    /// The JSON request body (only when `Include::request_body`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
    /// The messages sent to the model (only when `Include::request_messages`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<Message>>,
}

/// Response metadata of a step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepResponse {
    /// Provider response id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Response timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    /// Model that produced the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<ModelId>,
    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
    /// Raw response body (only when `Include::response_body`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
    /// Messages produced by the step (assistant and tool messages).
    #[serde(default)]
    pub messages: Vec<Message>,
}

/// Timing and throughput of a step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepPerformance {
    /// Wall time of the whole step (model call and tool executions).
    #[serde(default)]
    pub step_time: Duration,
    /// Time from the request until the response (or stream end).
    #[serde(default)]
    pub response_time: Duration,
    /// Streaming: time until the first content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_output: Option<Duration>,
    /// Streaming: output tokens per second after the first output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_per_second: Option<f64>,
    /// Output tokens divided by the response time.
    #[serde(default)]
    pub effective_output_tokens_per_second: f64,
    /// Streaming: input tokens divided by the time to first output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_per_second: Option<f64>,
    /// Input plus output tokens divided by the response time.
    #[serde(default)]
    pub effective_total_tokens_per_second: f64,
    /// Streaming: statistics of the gaps between content parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_between_output_chunks: Option<ChunkTimingStats>,
}

impl StepPerformance {
    /// Tokens per second, `0.0` when the duration is zero.
    #[must_use]
    pub fn tokens_per_second(tokens: Option<u64>, duration: Duration) -> f64 {
        let seconds = duration.as_secs_f64();
        if seconds <= 0.0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "token counts fit in f64 for rates"
        )]
        let rate = tokens.unwrap_or(0) as f64 / seconds;
        if rate.is_finite() { rate } else { 0.0 }
    }
}

/// Statistics over the gaps between consecutive content parts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChunkTimingStats {
    /// Smallest gap.
    pub min: Duration,
    /// Largest gap.
    pub max: Duration,
    /// Mean gap.
    pub mean: Duration,
    /// Median gap.
    pub p50: Duration,
    /// 90th percentile.
    pub p90: Duration,
    /// 99th percentile.
    pub p99: Duration,
    /// Number of gaps.
    pub count: u64,
}

impl ChunkTimingStats {
    /// Computes the statistics of `gaps`; `None` when empty.
    #[must_use]
    pub fn from_gaps(gaps: &[Duration]) -> Option<Self> {
        if gaps.is_empty() {
            return None;
        }
        let mut sorted = gaps.to_vec();
        sorted.sort_unstable();
        let total: Duration = sorted.iter().sum();
        let count = sorted.len();
        let percentile = |p: f64| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "index arithmetic on a small vector"
            )]
            let index = ((p / 100.0) * (count as f64 - 1.0)).round() as usize;
            sorted[index.min(count - 1)]
        };
        Some(Self {
            min: sorted[0],
            max: sorted[count - 1],
            mean: total / u32::try_from(count).unwrap_or(u32::MAX),
            p50: percentile(50.0),
            p90: percentile(90.0),
            p99: percentile(99.0),
            count: count as u64,
        })
    }
}

/// A content part of a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum StepContent {
    /// Generated text.
    Text {
        /// The text.
        text: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Reasoning text.
    Reasoning {
        /// The text.
        text: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A reasoning artifact stored as a file.
    ReasoningFile(GeneratedFile),
    /// A generated file.
    File(GeneratedFile),
    /// Provider-specific content.
    Custom {
        /// Kind of the content.
        kind: CustomKind,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A citation source.
    Source(Source),
    /// A parsed tool call.
    ToolCall(ParsedToolCall),
    /// A tool result (client- or provider-executed).
    ToolResult(ToolResult),
    /// A tool error (client- or provider-executed).
    ToolError(ToolExecutionError),
    /// A request to approve a tool call.
    ToolApprovalRequest(ToolApprovalRequestContent),
    /// An automatic approval decision made by the approval policy.
    ToolApprovalResponse(ToolApprovalResponseContent),
    /// A tool call whose execution was denied.
    ToolOutputDenied(ToolOutputDenied),
}

impl StepContent {
    /// Creates a text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Returns the wire name of the variant.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Reasoning { .. } => "reasoning",
            Self::ReasoningFile(_) => "reasoning-file",
            Self::File(_) => "file",
            Self::Custom { .. } => "custom",
            Self::Source(_) => "source",
            Self::ToolCall(_) => "tool-call",
            Self::ToolResult(_) => "tool-result",
            Self::ToolError(_) => "tool-error",
            Self::ToolApprovalRequest(_) => "tool-approval-request",
            Self::ToolApprovalResponse(_) => "tool-approval-response",
            Self::ToolOutputDenied(_) => "tool-output-denied",
        }
    }
}

/// A generated file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// File payload (inline bytes or URL).
    pub data: FileData,
    /// Media type.
    pub media_type: MediaType,
    /// Optional file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl GeneratedFile {
    /// Returns the inline bytes when the payload is inline.
    #[must_use]
    pub fn bytes(&self) -> Option<&bytes::Bytes> {
        self.data.as_bytes()
    }

    /// Returns the payload as base64 when it is inline.
    #[must_use]
    pub fn base64(&self) -> Option<String> {
        self.data.to_base64()
    }
}

/// A tool call after parsing and validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedToolCall {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// Parsed input (the raw text when it was not valid JSON).
    pub input: JsonValue,
    /// Whether the provider executes the tool itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Whether the call targets a dynamic tool (or could not be validated).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Whether parsing or validation failed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invalid: bool,
    /// The parse or validation error message for invalid calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Display title of the tool, if defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Metadata from the tool definition, such as its MCP origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ParsedToolCall {
    /// Creates a valid, client-executed, static call.
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<ToolCallId>,
        tool_name: impl Into<ToolName>,
        input: JsonValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            provider_executed: false,
            dynamic: false,
            invalid: false,
            error: None,
            title: None,
            tool_metadata: None,
            provider_metadata: None,
        }
    }
}

/// A tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// The validated input.
    pub input: JsonValue,
    /// The output value.
    pub output: JsonValue,
    /// Whether the provider executed the tool.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Whether this result will be superseded by a final one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preliminary: bool,
    /// Execution time in milliseconds (client-executed tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<u64>,
    /// Metadata from the tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// A failed tool execution (non-fatal: reported back to the model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionError {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// The input.
    pub input: JsonValue,
    /// The error.
    pub error: ToolErrorInfo,
    /// Whether the provider executed the tool.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Metadata from the tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Serializable description of a tool error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ToolErrorInfo {
    /// A textual error.
    Text {
        /// The message.
        message: String,
    },
    /// A structured error payload.
    Json {
        /// The payload.
        value: JsonValue,
    },
}

impl ToolErrorInfo {
    /// Creates a textual error.
    #[must_use]
    pub fn text(message: impl Into<String>) -> Self {
        Self::Text {
            message: message.into(),
        }
    }

    /// The error as a JSON value (text becomes a JSON string).
    #[must_use]
    pub fn to_json_value(&self) -> JsonValue {
        match self {
            Self::Text { message } => JsonValue::String(message.clone()),
            Self::Json { value } => value.clone(),
        }
    }

    /// A human-readable message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Text { message } => message.clone(),
            Self::Json { value } => ferrin_tool::model_output::error_message(value),
        }
    }
}

impl From<&ToolError> for ToolErrorInfo {
    fn from(error: &ToolError) -> Self {
        match error {
            ToolError::Json { value } => Self::Json {
                value: value.clone(),
            },
            other => Self::text(other.to_string()),
        }
    }
}

impl std::fmt::Display for ToolErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// A request to approve a tool call before it executes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolApprovalRequestContent {
    /// Identifier of the approval request.
    pub approval_id: ApprovalId,
    /// The tool call awaiting approval.
    pub tool_call: ParsedToolCall,
    /// Reason shown to the approver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the decision was made automatically by the approval policy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_automatic: bool,
    /// HMAC signature over the request (when a secret is configured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Provider-specific metadata (provider-issued requests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// An approval decision made automatically by the approval policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolApprovalResponseContent {
    /// Identifier of the approval request.
    pub approval_id: ApprovalId,
    /// The tool call the decision concerns.
    pub tool_call: ParsedToolCall,
    /// Whether execution was approved.
    pub approved: bool,
    /// Reason given by the policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the provider executes the tool.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
}

/// A tool call whose execution was denied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutputDenied {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// The input.
    pub input: JsonValue,
    /// Why execution was denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the provider would have executed the tool.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Metadata from the current tool definition when approval was replayed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
    /// Provider routing metadata from the original tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}
