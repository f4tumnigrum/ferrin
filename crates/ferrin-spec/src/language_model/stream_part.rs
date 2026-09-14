//! Stream parts emitted by `do_stream`.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::content::CustomKind;
use super::content::ProviderToolResult;
use super::content::Source;
use super::content::ToolCall;
use super::finish_reason::FinishReason;
use super::usage::Usage;
use crate::error::ProviderError;
use crate::json::JsonValue;
use crate::shared::ApprovalId;
use crate::shared::FileData;
use crate::shared::MediaType;
use crate::shared::ModelId;
use crate::shared::PartId;
use crate::shared::ProviderMetadata;
use crate::shared::ToolCallId;
use crate::shared::ToolName;
use crate::shared::Warning;

/// A part of a language model stream, tagged by `type`.
///
/// Ordering contract: a stream starts with `StreamStart` and ends with
/// `Finish` (or `Error`); `TextDelta` parts appear between `TextStart` and
/// `TextEnd` with the same id; tool input appears as `ToolInputStart`, zero
/// or more `ToolInputDelta`, `ToolInputEnd`, then `ToolCall`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum StreamPart {
    /// First part of every stream.
    StreamStart {
        /// Warnings produced while preparing the call.
        #[serde(default)]
        warnings: Vec<Warning>,
    },
    /// Response metadata, emitted once known.
    ResponseMetadata {
        /// Provider response id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Response timestamp.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp: Option<DateTime<Utc>>,
        /// Model that produced the response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_id: Option<ModelId>,
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
        delta: String,
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
        /// Appended reasoning text.
        delta: String,
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
    /// Start of streamed tool input.
    ToolInputStart {
        /// Tool call id.
        id: ToolCallId,
        /// Name of the tool.
        tool_name: ToolName,
        /// Whether the provider executes the tool itself.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        provider_executed: bool,
        /// Whether the tool is dynamic.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dynamic: bool,
        /// Human-readable title for display, if the provider supplies one.
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
    /// A complete tool call.
    ToolCall(ToolCall),
    /// A provider-executed tool result.
    ToolResult(ProviderToolResult),
    /// The provider asks for approval before executing a tool call.
    ToolApprovalRequest {
        /// Identifier of the approval request.
        approval_id: ApprovalId,
        /// Identifier of the tool call awaiting approval.
        tool_call_id: ToolCallId,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A generated file.
    File {
        /// File payload.
        data: FileData,
        /// Media type of the payload.
        media_type: MediaType,
        /// Optional file name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A reasoning artifact stored as a file.
    ReasoningFile {
        /// File payload.
        data: FileData,
        /// Media type of the payload.
        media_type: MediaType,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A citation source.
    Source(Source),
    /// Provider-specific content.
    Custom {
        /// Kind of the content.
        kind: CustomKind,
        /// Provider-specific metadata carrying the payload.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Last part of a successful stream.
    Finish {
        /// Why generation stopped.
        finish_reason: FinishReason,
        /// Token usage of the call.
        usage: Usage,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A raw provider chunk; only emitted when `include_raw_chunks` is set.
    Raw {
        /// The provider chunk as JSON.
        raw_value: JsonValue,
    },
    /// An error; the stream ends after this part.
    Error {
        /// The error.
        error: StreamError,
    },
}

impl StreamPart {
    /// Creates a `StreamStart` without warnings.
    #[must_use]
    pub fn stream_start() -> Self {
        Self::StreamStart {
            warnings: Vec::new(),
        }
    }

    /// Creates a `TextDelta` without metadata.
    #[must_use]
    pub fn text_delta(id: impl Into<PartId>, delta: impl Into<String>) -> Self {
        Self::TextDelta {
            id: id.into(),
            delta: delta.into(),
            provider_metadata: None,
        }
    }

    /// Creates a `Finish` without metadata.
    #[must_use]
    pub fn finish(finish_reason: FinishReason, usage: Usage) -> Self {
        Self::Finish {
            finish_reason,
            usage,
            provider_metadata: None,
        }
    }

    /// Creates an `Error` part from a provider error.
    #[must_use]
    pub fn error(error: &ProviderError) -> Self {
        Self::Error {
            error: StreamError::from_provider_error(error),
        }
    }

    /// Returns the wire name of the variant (`text-delta`, `finish`, ...).
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::StreamStart { .. } => "stream-start",
            Self::ResponseMetadata { .. } => "response-metadata",
            Self::TextStart { .. } => "text-start",
            Self::TextDelta { .. } => "text-delta",
            Self::TextEnd { .. } => "text-end",
            Self::ReasoningStart { .. } => "reasoning-start",
            Self::ReasoningDelta { .. } => "reasoning-delta",
            Self::ReasoningEnd { .. } => "reasoning-end",
            Self::ToolInputStart { .. } => "tool-input-start",
            Self::ToolInputDelta { .. } => "tool-input-delta",
            Self::ToolInputEnd { .. } => "tool-input-end",
            Self::ToolCall(_) => "tool-call",
            Self::ToolResult(_) => "tool-result",
            Self::ToolApprovalRequest { .. } => "tool-approval-request",
            Self::File { .. } => "file",
            Self::ReasoningFile { .. } => "reasoning-file",
            Self::Source(_) => "source",
            Self::Custom { .. } => "custom",
            Self::Finish { .. } => "finish",
            Self::Raw { .. } => "raw",
            Self::Error { .. } => "error",
        }
    }
}

/// Serializable error carried inside a stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamError {
    /// Human-readable message.
    pub message: String,
    /// Provider error type, if reported.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    /// Provider error code, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<StreamErrorCode>,
    /// HTTP status code, if the error came from an HTTP response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    /// Whether retrying the call may succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_retryable: Option<bool>,
    /// Raw provider error payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
}

impl StreamError {
    /// Creates a stream error with only a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: None,
            code: None,
            status_code: None,
            is_retryable: None,
            data: None,
        }
    }

    /// Projects a [`ProviderError`] into a stream error.
    #[must_use]
    pub fn from_provider_error(error: &ProviderError) -> Self {
        let mut stream_error = Self::new(error.to_string());
        stream_error.is_retryable = Some(error.is_retryable());
        stream_error.status_code = error.status_code().map(|status| status.as_u16());
        if let ProviderError::ApiCall(api) = error {
            stream_error.data = api.data.clone();
        }
        stream_error
    }
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StreamError {}

/// Provider error code: a string or a number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamErrorCode {
    /// Textual code.
    Text(String),
    /// Numeric code.
    Number(i64),
}

impl std::fmt::Display for StreamErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(text) => f.write_str(text),
            Self::Number(number) => write!(f, "{number}"),
        }
    }
}
