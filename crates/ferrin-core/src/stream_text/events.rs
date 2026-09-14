//! Events emitted by `stream_text`.

use ferrin_spec::CustomKind;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::Source;
use serde::Deserialize;
use serde::Serialize;

use crate::error::Error;
use crate::error::ErrorKind;
use crate::generate_text::GeneratedFile;
use crate::generate_text::ParsedToolCall;
use crate::generate_text::StepPerformance;
use crate::generate_text::StepRequest;
use crate::generate_text::StepResponse;
use crate::generate_text::ToolApprovalRequestContent;
use crate::generate_text::ToolApprovalResponseContent;
use crate::generate_text::ToolExecutionError;
use crate::generate_text::ToolOutputDenied;
use crate::generate_text::ToolResult;
use crate::telemetry::ModelIdentity;

/// One event of a text stream. Serializable, so applications can forward
/// events over SSE or WebSocket frames unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
#[allow(
    clippy::large_enum_variant,
    reason = "events flow through channels one at a time; flat variants keep pattern matching simple"
)]
pub enum StreamEvent {
    /// The call started.
    Start {
        /// Call id.
        call_id: String,
    },
    /// A step started.
    StartStep {
        /// Zero-based step index.
        step_number: u32,
        /// The model handling the step.
        model: ModelIdentity,
        /// Request metadata.
        request: StepRequest,
        /// Warnings from the adapter.
        warnings: Vec<Warning>,
    },
    /// Start of a text part.
    TextStart {
        /// Part id.
        id: PartId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Text increment.
    TextDelta {
        /// Part id.
        id: PartId,
        /// Appended text.
        text: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// End of a text part.
    TextEnd {
        /// Part id.
        id: PartId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Start of a reasoning part.
    ReasoningStart {
        /// Part id.
        id: PartId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Reasoning increment.
    ReasoningDelta {
        /// Part id.
        id: PartId,
        /// Appended text.
        text: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// End of a reasoning part.
    ReasoningEnd {
        /// Part id.
        id: PartId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A reasoning artifact stored as a file.
    ReasoningFile(GeneratedFile),
    /// A generated file.
    File(GeneratedFile),
    /// A citation source.
    Source(Source),
    /// Provider-specific content.
    Custom {
        /// Kind of the content.
        kind: CustomKind,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Start of streamed tool input.
    ToolInputStart {
        /// Tool call id.
        id: ToolCallId,
        /// Tool name.
        tool_name: ToolName,
        /// Whether the provider executes the tool.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        provider_executed: bool,
        /// Whether the tool is dynamic.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dynamic: bool,
        /// Display title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Tool input increment (raw JSON text).
    ToolInputDelta {
        /// Tool call id.
        id: ToolCallId,
        /// Appended JSON text.
        delta: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// End of streamed tool input.
    ToolInputEnd {
        /// Tool call id.
        id: ToolCallId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A parsed tool call.
    ToolCall(ParsedToolCall),
    /// A tool result (may be preliminary).
    ToolResult(ToolResult),
    /// A tool error.
    ToolError(ToolExecutionError),
    /// A request to approve a tool call.
    ToolApprovalRequest(ToolApprovalRequestContent),
    /// An automatic approval decision.
    ToolApprovalResponse(ToolApprovalResponseContent),
    /// A denied tool call.
    ToolOutputDenied(ToolOutputDenied),
    /// A step finished.
    FinishStep {
        /// Zero-based step index.
        step_number: u32,
        /// Finish reason.
        finish_reason: FinishReason,
        /// Usage of the step.
        usage: Usage,
        /// Response metadata (without messages).
        response: StepResponse,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
        /// Timing and throughput of the step.
        performance: StepPerformance,
    },
    /// The call finished.
    Finish {
        /// Finish reason of the last step.
        finish_reason: FinishReason,
        /// Usage summed over all steps.
        total_usage: Usage,
    },
    /// An error occurred (the stream may continue after a retry).
    Error {
        /// The error.
        error: StreamErrorInfo,
    },
    /// The call was aborted by cancellation.
    Abort,
    /// The model call of the current step was retried after a stream error.
    /// Content emitted by the previous attempt stays in the stream but is
    /// excluded from the step result.
    RetryAttempt {
        /// Zero-based step index.
        step_number: u32,
        /// One-based retry attempt number.
        attempt: u32,
        /// Request metadata of the new attempt.
        request: StepRequest,
        /// Warnings from the new attempt's stream start.
        warnings: Vec<Warning>,
    },
    /// A raw provider chunk (only with `include_raw_chunks`).
    Raw {
        /// The chunk.
        raw_value: JsonValue,
    },
}

impl StreamEvent {
    /// Returns the wire name of the variant.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start",
            Self::StartStep { .. } => "start-step",
            Self::TextStart { .. } => "text-start",
            Self::TextDelta { .. } => "text-delta",
            Self::TextEnd { .. } => "text-end",
            Self::ReasoningStart { .. } => "reasoning-start",
            Self::ReasoningDelta { .. } => "reasoning-delta",
            Self::ReasoningEnd { .. } => "reasoning-end",
            Self::ReasoningFile(_) => "reasoning-file",
            Self::File(_) => "file",
            Self::Source(_) => "source",
            Self::Custom { .. } => "custom",
            Self::ToolInputStart { .. } => "tool-input-start",
            Self::ToolInputDelta { .. } => "tool-input-delta",
            Self::ToolInputEnd { .. } => "tool-input-end",
            Self::ToolCall(_) => "tool-call",
            Self::ToolResult(_) => "tool-result",
            Self::ToolError(_) => "tool-error",
            Self::ToolApprovalRequest(_) => "tool-approval-request",
            Self::ToolApprovalResponse(_) => "tool-approval-response",
            Self::ToolOutputDenied(_) => "tool-output-denied",
            Self::FinishStep { .. } => "finish-step",
            Self::Finish { .. } => "finish",
            Self::Error { .. } => "error",
            Self::Abort => "abort",
            Self::RetryAttempt { .. } => "retry-attempt",
            Self::Raw { .. } => "raw",
        }
    }

    /// Returns the text of a text delta.
    #[must_use]
    pub fn as_text_delta(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text, .. } => Some(text),
            _ => None,
        }
    }
}

/// Serializable projection of an [`Error`] carried in a stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamErrorInfo {
    /// Coarse category.
    pub kind: ErrorKind,
    /// Display message.
    pub message: String,
    /// HTTP status when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    /// Whether retrying may succeed.
    pub is_retryable: bool,
    /// Provider error payload when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_data: Option<JsonValue>,
}

impl StreamErrorInfo {
    /// Projects an error.
    #[must_use]
    pub fn from_error(error: &Error) -> Self {
        let provider_data = error
            .as_provider()
            .and_then(ferrin_spec::error::ProviderError::as_api_call)
            .and_then(|api| api.data.clone());
        Self {
            kind: error.kind(),
            message: error.to_string(),
            status_code: error.status_code().map(|status| status.as_u16()),
            is_retryable: error.is_retryable(),
            provider_data,
        }
    }
}

impl From<&Error> for StreamErrorInfo {
    fn from(error: &Error) -> Self {
        Self::from_error(error)
    }
}
