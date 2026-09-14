//! Content produced by a language model call.

use serde::Deserialize;
use serde::Serialize;

use crate::json::JsonValue;
use crate::shared::ApprovalId;
use crate::shared::FileData;
use crate::shared::MediaType;
use crate::shared::ProviderMetadata;
use crate::shared::ToolCallId;
use crate::shared::ToolName;

/// A content part of a generation result, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Content {
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
        /// The reasoning text.
        text: String,
        /// Provider-specific metadata (signatures, encrypted state, ...).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A reasoning artifact stored as a file.
    ReasoningFile {
        /// File payload (inline bytes or URL).
        data: FileData,
        /// Media type of the payload.
        media_type: MediaType,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A generated file (image, audio, ...).
    File {
        /// File payload (inline bytes or URL).
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
    /// Provider-specific content identified by a `provider.type` kind.
    Custom {
        /// Kind of the content.
        kind: CustomKind,
        /// Provider-specific metadata carrying the payload.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A citation source.
    Source(Source),
    /// A tool call requested by the model.
    ToolCall(ToolCall),
    /// The result of a provider-executed tool.
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
}

impl Content {
    /// Creates a text part without metadata.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Creates a reasoning part without metadata.
    #[must_use]
    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning {
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Returns the text of a [`Content::Text`] part.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Returns the tool call of a [`Content::ToolCall`] part.
    #[must_use]
    pub fn as_tool_call(&self) -> Option<&ToolCall> {
        match self {
            Self::ToolCall(call) => Some(call),
            _ => None,
        }
    }

    /// Returns the wire name of the variant (`text`, `tool-call`, ...).
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Reasoning { .. } => "reasoning",
            Self::ReasoningFile { .. } => "reasoning-file",
            Self::File { .. } => "file",
            Self::Custom { .. } => "custom",
            Self::Source(_) => "source",
            Self::ToolCall(_) => "tool-call",
            Self::ToolResult(_) => "tool-result",
            Self::ToolApprovalRequest { .. } => "tool-approval-request",
        }
    }
}

/// A tool call requested by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// Raw JSON text as emitted by the provider; parsed and validated by the core.
    pub input: String,
    /// Whether the provider executes the tool itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
    /// Whether the tool was not part of the static tool set (dynamic tool).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolCall {
    /// Creates a client-executed, static tool call.
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<ToolCallId>,
        tool_name: impl Into<ToolName>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            provider_executed: false,
            dynamic: false,
            provider_metadata: None,
        }
    }
}

/// The result of a tool executed by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderToolResult {
    /// Identifier of the tool call.
    pub tool_call_id: ToolCallId,
    /// Name of the tool.
    pub tool_name: ToolName,
    /// The result value.
    pub result: JsonValue,
    /// Whether the result is an error.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
    /// Whether this is a preliminary result that will be superseded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preliminary: bool,
    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// A citation source, tagged by `source_type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source_type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Source {
    /// A web URL.
    Url {
        /// Provider-assigned source id.
        id: String,
        /// The URL as reported by the provider.
        url: String,
        /// Optional title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A document.
    Document {
        /// Provider-assigned source id.
        id: String,
        /// Media type of the document.
        media_type: MediaType,
        /// Title of the document.
        title: String,
        /// Optional file name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

/// Kind of a custom content part, in `provider.type` form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CustomKind(String);

impl CustomKind {
    /// Parses a kind; it must be `<provider>.<type>` with non-empty halves.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCustomKind`] when the text does not contain exactly
    /// one separating dot with non-empty text on both sides.
    pub fn parse(text: impl Into<String>) -> Result<Self, InvalidCustomKind> {
        let text = text.into();
        match text.split_once('.') {
            Some((provider, kind))
                if !provider.is_empty()
                    && !kind.is_empty()
                    && !provider.contains(char::is_whitespace)
                    && !kind.contains(char::is_whitespace) =>
            {
                Ok(Self(text))
            }
            _ => Err(InvalidCustomKind { text }),
        }
    }

    /// Builds a kind from its two halves.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCustomKind`] when either half is empty or contains
    /// whitespace or a dot.
    pub fn new(provider: &str, kind: &str) -> Result<Self, InvalidCustomKind> {
        if provider.contains('.') {
            return Err(InvalidCustomKind {
                text: format!("{provider}.{kind}"),
            });
        }
        Self::parse(format!("{provider}.{kind}"))
    }

    /// Returns the provider half (`openai` in `openai.web_search`).
    #[must_use]
    pub fn provider(&self) -> &str {
        self.0.split_once('.').map_or("", |(provider, _)| provider)
    }

    /// Returns the type half (`web_search` in `openai.web_search`).
    #[must_use]
    pub fn kind(&self) -> &str {
        self.0.split_once('.').map_or("", |(_, kind)| kind)
    }

    /// Returns the full `provider.type` string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CustomKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CustomKind {
    type Error = InvalidCustomKind;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<CustomKind> for String {
    fn from(kind: CustomKind) -> Self {
        kind.0
    }
}

impl AsRef<str> for CustomKind {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Error returned when a custom kind is not of the form `provider.type`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid custom kind `{text}`: expected `<provider>.<type>`")]
pub struct InvalidCustomKind {
    /// The rejected text.
    pub text: String,
}
