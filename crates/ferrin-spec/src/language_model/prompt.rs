//! Specification-level prompt: the message list sent to a language model.
//!
//! The core converts application messages (`ferrin-message`) into this form,
//! downloading files the provider cannot fetch and validating structure.
//! Adapters convert it into provider-specific request bodies.

use serde::Deserialize;
use serde::Serialize;

use crate::json::JsonValue;
use crate::shared::ApprovalId;
use crate::shared::FileData;
use crate::shared::MediaType;
use crate::shared::ProviderOptions;
use crate::shared::ToolCallId;
use crate::shared::ToolName;

use super::content::CustomKind;

/// A prompt is an ordered list of messages.
pub type Prompt = Vec<PromptMessage>;

/// A message in a specification prompt, tagged by `role`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
#[non_exhaustive]
pub enum PromptMessage {
    /// System instructions.
    System {
        /// Instruction text.
        content: String,
        /// Provider-specific options for this message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// User input.
    User {
        /// Text and file parts.
        content: Vec<UserPromptPart>,
        /// Provider-specific options for this message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// Previous assistant output.
    Assistant {
        /// Assistant parts, including tool calls and provider-executed results.
        content: Vec<AssistantPromptPart>,
        /// Provider-specific options for this message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// Tool results and approval responses.
    Tool {
        /// Tool parts.
        content: Vec<ToolPromptPart>,
        /// Provider-specific options for this message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

impl PromptMessage {
    /// Creates a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
            provider_options: None,
        }
    }

    /// Creates a user message with a single text part.
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::User {
            content: vec![UserPromptPart::Text(TextPart::new(text))],
            provider_options: None,
        }
    }

    /// Creates a user message from parts.
    #[must_use]
    pub fn user(content: Vec<UserPromptPart>) -> Self {
        Self::User {
            content,
            provider_options: None,
        }
    }

    /// Creates an assistant message with a single text part.
    #[must_use]
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::Assistant {
            content: vec![AssistantPromptPart::Text(TextPart::new(text))],
            provider_options: None,
        }
    }

    /// Creates an assistant message from parts.
    #[must_use]
    pub fn assistant(content: Vec<AssistantPromptPart>) -> Self {
        Self::Assistant {
            content,
            provider_options: None,
        }
    }

    /// Creates a tool message from parts.
    #[must_use]
    pub fn tool(content: Vec<ToolPromptPart>) -> Self {
        Self::Tool {
            content,
            provider_options: None,
        }
    }

    /// Returns the role name (`system`, `user`, `assistant`, `tool`).
    #[must_use]
    pub fn role(&self) -> &'static str {
        match self {
            Self::System { .. } => "system",
            Self::User { .. } => "user",
            Self::Assistant { .. } => "assistant",
            Self::Tool { .. } => "tool",
        }
    }

    /// Returns the provider options attached to the message.
    #[must_use]
    pub fn provider_options(&self) -> Option<&ProviderOptions> {
        match self {
            Self::System {
                provider_options, ..
            }
            | Self::User {
                provider_options, ..
            }
            | Self::Assistant {
                provider_options, ..
            }
            | Self::Tool {
                provider_options, ..
            } => provider_options.as_ref(),
        }
    }
}

/// Parts allowed in a user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum UserPromptPart {
    /// Text.
    Text(TextPart),
    /// A file (image, audio, document, ...).
    File(FilePart),
}

/// Parts allowed in an assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum AssistantPromptPart {
    /// Text.
    Text(TextPart),
    /// A generated file.
    File(FilePart),
    /// Reasoning text.
    Reasoning(ReasoningPart),
    /// A reasoning artifact stored as a file.
    ReasoningFile(ReasoningFilePart),
    /// Provider-specific content identified by kind.
    Custom(CustomPart),
    /// A tool call issued by the model.
    ToolCall(ToolCallPart),
    /// A provider-executed tool result.
    ToolResult(ToolResultPart),
}

/// Parts allowed in a tool message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ToolPromptPart {
    /// Result of a client-executed tool call.
    ToolResult(ToolResultPart),
    /// Response to a tool approval request.
    ToolApprovalResponse(ToolApprovalResponsePart),
}

/// Text content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPart {
    /// The text.
    pub text: String,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl TextPart {
    /// Creates a text part without provider options.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            provider_options: None,
        }
    }
}

/// Reasoning text produced by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningPart {
    /// The reasoning text.
    pub text: String,
    /// Provider-specific options (for example signatures or encrypted state).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl ReasoningPart {
    /// Creates a reasoning part without provider options.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            provider_options: None,
        }
    }
}

/// A reasoning artifact stored as a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningFilePart {
    /// File payload (inline bytes or URL).
    pub data: FileData,
    /// Media type of the payload.
    pub media_type: MediaType,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// Provider-specific content identified by a `provider.type` kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPart {
    /// Kind of the content, in `provider.type` form.
    pub kind: CustomKind,
    /// Provider-specific options carrying the actual payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A file attached to a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePart {
    /// File payload.
    pub data: FileData,
    /// Full media type (`image/png`) or top-level type (`image`).
    pub media_type: MediaType,
    /// Optional file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl FilePart {
    /// Creates a file part without name or provider options.
    #[must_use]
    pub fn new(data: impl Into<FileData>, media_type: impl Into<MediaType>) -> Self {
        Self {
            data: data.into(),
            media_type: media_type.into(),
            filename: None,
            provider_options: None,
        }
    }

    /// Sets the file name.
    #[must_use]
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

/// A tool call issued by the model in a previous step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallPart {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// Parsed tool input.
    pub input: JsonValue,
    /// Whether the provider executed the tool itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// The result of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultPart {
    /// Identifier of the tool call this result answers.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// The output.
    pub output: ToolResultOutput,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A response to a tool approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalResponsePart {
    /// Identifier of the approval request.
    pub approval_id: ApprovalId,
    /// Whether execution was approved.
    pub approved: bool,
    /// Optional reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Provider-specific options for this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// The output of a tool call as sent back to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ToolResultOutput {
    /// Plain text.
    Text {
        /// The text.
        value: String,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// A JSON value.
    Json {
        /// The value.
        value: JsonValue,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// Execution was denied by the application or user.
    ExecutionDenied {
        /// Optional reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// The tool failed; the error is plain text.
    ErrorText {
        /// The error text.
        value: String,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// The tool failed; the error is a JSON value.
    ErrorJson {
        /// The error value.
        value: JsonValue,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// Multi-part content (text and files).
    Content {
        /// The parts.
        value: Vec<ToolResultContentPart>,
    },
}

impl ToolResultOutput {
    /// Creates a text output.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
            provider_options: None,
        }
    }

    /// Creates a JSON output.
    #[must_use]
    pub fn json(value: JsonValue) -> Self {
        Self::Json {
            value,
            provider_options: None,
        }
    }

    /// Creates an error-text output.
    #[must_use]
    pub fn error_text(value: impl Into<String>) -> Self {
        Self::ErrorText {
            value: value.into(),
            provider_options: None,
        }
    }

    /// Creates an error-JSON output.
    #[must_use]
    pub fn error_json(value: JsonValue) -> Self {
        Self::ErrorJson {
            value,
            provider_options: None,
        }
    }

    /// Creates an execution-denied output.
    #[must_use]
    pub fn execution_denied(reason: Option<String>) -> Self {
        Self::ExecutionDenied {
            reason,
            provider_options: None,
        }
    }

    /// Returns `true` for the error variants (`error-text`, `error-json`).
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::ErrorText { .. } | Self::ErrorJson { .. })
    }
}

/// A part of a multi-part tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ToolResultContentPart {
    /// Text.
    Text {
        /// The text.
        text: String,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// A file.
    File {
        /// File payload.
        data: FileData,
        /// Media type of the payload.
        media_type: MediaType,
        /// Optional file name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// Provider-specific content carried entirely in provider options.
    Custom {
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}
