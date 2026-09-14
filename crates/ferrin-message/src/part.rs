//! Content parts of application messages.

use std::path::PathBuf;

use bytes::Bytes;
use ferrin_spec::ApprovalId;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ProviderReference;
use ferrin_spec::ToolCallId;
use ferrin_spec::language_model::prompt::CustomPart;
use ferrin_spec::language_model::prompt::ReasoningPart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultPart;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use crate::file_source::FileSource;

/// A part of a user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum UserPart {
    /// Text.
    Text(TextPart),
    /// An image; normalized to a file part with a detected media type during
    /// conversion.
    Image(ImagePart),
    /// A file with an explicit media type.
    File(FilePart),
}

impl UserPart {
    /// A text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextPart::new(text))
    }

    /// An image from any source.
    #[must_use]
    pub fn image(source: impl Into<FileSource>) -> Self {
        Self::Image(ImagePart::new(source))
    }

    /// An image from inline bytes.
    #[must_use]
    pub fn image_bytes(data: impl Into<Bytes>) -> Self {
        Self::image(FileSource::bytes(data))
    }

    /// An image from base64 text.
    #[must_use]
    pub fn image_base64(data: impl Into<String>) -> Self {
        Self::image(FileSource::base64(data))
    }

    /// An image from a URL.
    #[must_use]
    pub fn image_url(url: Url) -> Self {
        Self::image(FileSource::url(url))
    }

    /// A file from any source.
    #[must_use]
    pub fn file(source: impl Into<FileSource>, media_type: impl Into<MediaType>) -> Self {
        Self::File(FilePart::new(source, media_type))
    }

    /// A file from inline bytes.
    #[must_use]
    pub fn file_bytes(data: impl Into<Bytes>, media_type: impl Into<MediaType>) -> Self {
        Self::file(FileSource::bytes(data), media_type)
    }

    /// A file from a URL.
    #[must_use]
    pub fn file_url(url: Url, media_type: impl Into<MediaType>) -> Self {
        Self::file(FileSource::url(url), media_type)
    }

    /// A file previously uploaded to providers.
    #[must_use]
    pub fn file_reference(reference: ProviderReference, media_type: impl Into<MediaType>) -> Self {
        Self::file(FileSource::Reference { reference }, media_type)
    }

    /// An inline text document.
    #[must_use]
    pub fn file_text(text: impl Into<String>, media_type: impl Into<MediaType>) -> Self {
        Self::file(FileSource::text(text), media_type)
    }

    /// A file read from a local path during conversion.
    #[must_use]
    pub fn file_path(path: impl Into<PathBuf>, media_type: impl Into<MediaType>) -> Self {
        Self::file(FileSource::path(path), media_type)
    }

    /// Sets the filename on a file part (no effect on other parts).
    #[must_use]
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        if let Self::File(file) = &mut self {
            file.filename = Some(filename.into());
        }
        self
    }

    /// Sets provider options.
    #[must_use]
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        match &mut self {
            Self::Text(part) => part.provider_options = Some(options),
            Self::Image(part) => part.provider_options = Some(options),
            Self::File(part) => part.provider_options = Some(options),
        }
        self
    }

    /// Returns the text of a text part.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(part) => Some(&part.text),
            Self::Image(_) | Self::File(_) => None,
        }
    }
}

impl From<TextPart> for UserPart {
    fn from(part: TextPart) -> Self {
        Self::Text(part)
    }
}

impl From<ImagePart> for UserPart {
    fn from(part: ImagePart) -> Self {
        Self::Image(part)
    }
}

impl From<FilePart> for UserPart {
    fn from(part: FilePart) -> Self {
        Self::File(part)
    }
}

impl From<&str> for UserPart {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<String> for UserPart {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

/// An image in a user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagePart {
    /// Image source.
    pub image: FileSource,
    /// Media type, detected from the bytes when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl ImagePart {
    /// Creates an image part.
    #[must_use]
    pub fn new(image: impl Into<FileSource>) -> Self {
        Self {
            image: image.into(),
            media_type: None,
            provider_options: None,
        }
    }

    /// Sets the media type.
    #[must_use]
    pub fn with_media_type(mut self, media_type: impl Into<MediaType>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }
}

/// A file in a user or assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePart {
    /// File source.
    pub data: FileSource,
    /// Media type.
    pub media_type: MediaType,
    /// Optional filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl FilePart {
    /// Creates a file part.
    #[must_use]
    pub fn new(data: impl Into<FileSource>, media_type: impl Into<MediaType>) -> Self {
        Self {
            data: data.into(),
            media_type: media_type.into(),
            filename: None,
            provider_options: None,
        }
    }

    /// Sets the filename.
    #[must_use]
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

/// A reasoning artefact (for example an image the model reasoned over) in an
/// assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningFilePart {
    /// File source (inline bytes or URL).
    pub data: FileSource,
    /// Media type.
    pub media_type: MediaType,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A request for user approval of a tool call, recorded in an assistant
/// message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalRequest {
    /// Approval identifier.
    pub approval_id: ApprovalId,
    /// The tool call awaiting approval.
    pub tool_call_id: ToolCallId,
    /// Why approval is needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the request was created automatically (for example by a
    /// provider-executed tool) rather than by the tool's approval policy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_automatic: bool,
    /// Tamper-detection signature over the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ToolApprovalRequest {
    /// Creates a request.
    #[must_use]
    pub fn new(approval_id: impl Into<ApprovalId>, tool_call_id: impl Into<ToolCallId>) -> Self {
        Self {
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            reason: None,
            is_automatic: false,
            signature: None,
        }
    }

    /// Sets the reason.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Marks the request as automatic.
    #[must_use]
    pub fn automatic(mut self) -> Self {
        self.is_automatic = true;
        self
    }

    /// Sets the signature.
    #[must_use]
    pub fn with_signature(mut self, signature: impl Into<String>) -> Self {
        self.signature = Some(signature.into());
        self
    }
}

/// The user's answer to a [`ToolApprovalRequest`], recorded in a tool
/// message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalResponse {
    /// The approval being answered.
    pub approval_id: ApprovalId,
    /// Whether execution was approved.
    pub approved: bool,
    /// Optional explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the approved tool is executed by the provider.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_executed: bool,
}

impl ToolApprovalResponse {
    /// An approval.
    #[must_use]
    pub fn approved(approval_id: impl Into<ApprovalId>) -> Self {
        Self {
            approval_id: approval_id.into(),
            approved: true,
            reason: None,
            provider_executed: false,
        }
    }

    /// A denial.
    #[must_use]
    pub fn denied(approval_id: impl Into<ApprovalId>) -> Self {
        Self {
            approval_id: approval_id.into(),
            approved: false,
            reason: None,
            provider_executed: false,
        }
    }

    /// Sets the reason.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Marks the tool as provider-executed.
    #[must_use]
    pub fn provider_executed(mut self) -> Self {
        self.provider_executed = true;
        self
    }
}

/// A part of an assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum AssistantPart {
    /// Text.
    Text(TextPart),
    /// Provider-specific content identified by kind.
    Custom(CustomPart),
    /// A generated file.
    File(FilePart),
    /// Reasoning text.
    Reasoning(ReasoningPart),
    /// A reasoning artefact.
    ReasoningFile(ReasoningFilePart),
    /// A tool call.
    ToolCall(ToolCallPart),
    /// A provider-executed tool result.
    ToolResult(ToolResultPart),
    /// A pending approval request.
    ToolApprovalRequest(ToolApprovalRequest),
}

impl AssistantPart {
    /// A text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextPart::new(text))
    }

    /// A reasoning part.
    #[must_use]
    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning(ReasoningPart::new(text))
    }

    /// Returns the text of a text part.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(part) => Some(&part.text),
            _ => None,
        }
    }

    /// Returns the tool call when this is one.
    #[must_use]
    pub fn as_tool_call(&self) -> Option<&ToolCallPart> {
        match self {
            Self::ToolCall(part) => Some(part),
            _ => None,
        }
    }

    /// Returns the approval request when this is one.
    #[must_use]
    pub fn as_tool_approval_request(&self) -> Option<&ToolApprovalRequest> {
        match self {
            Self::ToolApprovalRequest(part) => Some(part),
            _ => None,
        }
    }
}

impl From<TextPart> for AssistantPart {
    fn from(part: TextPart) -> Self {
        Self::Text(part)
    }
}

impl From<CustomPart> for AssistantPart {
    fn from(part: CustomPart) -> Self {
        Self::Custom(part)
    }
}

impl From<FilePart> for AssistantPart {
    fn from(part: FilePart) -> Self {
        Self::File(part)
    }
}

impl From<ReasoningPart> for AssistantPart {
    fn from(part: ReasoningPart) -> Self {
        Self::Reasoning(part)
    }
}

impl From<ReasoningFilePart> for AssistantPart {
    fn from(part: ReasoningFilePart) -> Self {
        Self::ReasoningFile(part)
    }
}

impl From<ToolCallPart> for AssistantPart {
    fn from(part: ToolCallPart) -> Self {
        Self::ToolCall(part)
    }
}

impl From<ToolResultPart> for AssistantPart {
    fn from(part: ToolResultPart) -> Self {
        Self::ToolResult(part)
    }
}

impl From<ToolApprovalRequest> for AssistantPart {
    fn from(part: ToolApprovalRequest) -> Self {
        Self::ToolApprovalRequest(part)
    }
}

impl From<&str> for AssistantPart {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<String> for AssistantPart {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

/// A part of a tool message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ToolPart {
    /// A client-side tool result.
    ToolResult(ToolResultPart),
    /// An approval response; stripped before the prompt reaches the model.
    ToolApprovalResponse(ToolApprovalResponse),
}

impl ToolPart {
    /// Returns the tool result when this is one.
    #[must_use]
    pub fn as_tool_result(&self) -> Option<&ToolResultPart> {
        match self {
            Self::ToolResult(part) => Some(part),
            Self::ToolApprovalResponse(_) => None,
        }
    }

    /// Returns the approval response when this is one.
    #[must_use]
    pub fn as_tool_approval_response(&self) -> Option<&ToolApprovalResponse> {
        match self {
            Self::ToolApprovalResponse(part) => Some(part),
            Self::ToolResult(_) => None,
        }
    }
}

impl From<ToolResultPart> for ToolPart {
    fn from(part: ToolResultPart) -> Self {
        Self::ToolResult(part)
    }
}

impl From<ToolApprovalResponse> for ToolPart {
    fn from(part: ToolApprovalResponse) -> Self {
        Self::ToolApprovalResponse(part)
    }
}
