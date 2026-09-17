//! Application messages.

use ferrin_spec::ProviderOptions;
use serde::Deserialize;
use serde::Serialize;

use crate::part::AssistantPart;
use crate::part::ToolApprovalResponse;
use crate::part::ToolPart;
use crate::part::UserPart;

/// A message in an application conversation, tagged by `role`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
#[non_exhaustive]
pub enum Message {
    /// Instructions for the model.
    System(SystemMessage),
    /// Input from the user.
    User(UserMessage),
    /// Output from the model.
    Assistant(AssistantMessage),
    /// Tool results and approval responses.
    Tool(ToolMessage),
}

/// The role of a [`Message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
    /// System.
    System,
    /// User.
    User,
    /// Assistant.
    Assistant,
    /// Tool.
    Tool,
}

impl Role {
    /// The wire name of the role.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A system message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemMessage {
    /// Instruction text.
    pub content: String,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    /// Text or parts.
    pub content: UserContent,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// An assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// Text or parts.
    pub content: AssistantContent,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A tool message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolMessage {
    /// Tool results and approval responses.
    pub content: Vec<ToolPart>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// Content of a user message: a string or a list of parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    /// Plain text, equivalent to a single text part.
    Text(String),
    /// Parts.
    Parts(Vec<UserPart>),
}

impl UserContent {
    /// Returns `true` when the content has no text and no parts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::Parts(parts) => parts.is_empty(),
        }
    }

    /// Returns the plain text when the content is a string.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Parts(_) => None,
        }
    }

    /// Returns the parts, converting plain text into a single text part.
    #[must_use]
    pub fn into_parts(self) -> Vec<UserPart> {
        match self {
            Self::Text(text) => vec![UserPart::text(text)],
            Self::Parts(parts) => parts,
        }
    }
}

impl From<&str> for UserContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for UserContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Vec<UserPart>> for UserContent {
    fn from(parts: Vec<UserPart>) -> Self {
        Self::Parts(parts)
    }
}

/// Content of an assistant message: a string or a list of parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssistantContent {
    /// Plain text, equivalent to a single text part.
    Text(String),
    /// Parts.
    Parts(Vec<AssistantPart>),
}

impl AssistantContent {
    /// Returns `true` when the content has no text and no parts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::Parts(parts) => parts.is_empty(),
        }
    }

    /// Returns the plain text when the content is a string.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Parts(_) => None,
        }
    }

    /// Returns the parts, converting plain text into a single text part.
    #[must_use]
    pub fn into_parts(self) -> Vec<AssistantPart> {
        match self {
            Self::Text(text) => vec![AssistantPart::text(text)],
            Self::Parts(parts) => parts,
        }
    }

    /// Returns the parts when the content is a list.
    #[must_use]
    pub fn as_parts(&self) -> Option<&[AssistantPart]> {
        match self {
            Self::Text(_) => None,
            Self::Parts(parts) => Some(parts),
        }
    }
}

impl From<&str> for AssistantContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for AssistantContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Vec<AssistantPart>> for AssistantContent {
    fn from(parts: Vec<AssistantPart>) -> Self {
        Self::Parts(parts)
    }
}

impl Message {
    /// A system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::System(SystemMessage {
            content: content.into(),
            provider_options: None,
        })
    }

    /// A user message with plain text.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self::User(UserMessage {
            content: UserContent::Text(text.into()),
            provider_options: None,
        })
    }

    /// A user message with parts.
    #[must_use]
    pub fn user_parts(parts: impl IntoIterator<Item = impl Into<UserPart>>) -> Self {
        Self::User(UserMessage {
            content: UserContent::Parts(parts.into_iter().map(Into::into).collect()),
            provider_options: None,
        })
    }

    /// An assistant message with plain text.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::Assistant(AssistantMessage {
            content: AssistantContent::Text(text.into()),
            provider_options: None,
        })
    }

    /// An assistant message with parts.
    #[must_use]
    pub fn assistant_parts(parts: impl IntoIterator<Item = impl Into<AssistantPart>>) -> Self {
        Self::Assistant(AssistantMessage {
            content: AssistantContent::Parts(parts.into_iter().map(Into::into).collect()),
            provider_options: None,
        })
    }

    /// A tool message.
    #[must_use]
    pub fn tool(parts: impl IntoIterator<Item = impl Into<ToolPart>>) -> Self {
        Self::Tool(ToolMessage {
            content: parts.into_iter().map(Into::into).collect(),
            provider_options: None,
        })
    }

    /// The role.
    #[must_use]
    pub fn role(&self) -> Role {
        match self {
            Self::System(_) => Role::System,
            Self::User(_) => Role::User,
            Self::Assistant(_) => Role::Assistant,
            Self::Tool(_) => Role::Tool,
        }
    }

    /// Returns `true` when the message carries no content (empty string or
    /// no parts).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::System(message) => message.content.is_empty(),
            Self::User(message) => message.content.is_empty(),
            Self::Assistant(message) => message.content.is_empty(),
            Self::Tool(message) => message.content.is_empty(),
        }
    }

    /// Provider-specific options.
    #[must_use]
    pub fn provider_options(&self) -> Option<&ProviderOptions> {
        match self {
            Self::System(message) => message.provider_options.as_ref(),
            Self::User(message) => message.provider_options.as_ref(),
            Self::Assistant(message) => message.provider_options.as_ref(),
            Self::Tool(message) => message.provider_options.as_ref(),
        }
    }

    /// Sets provider-specific options.
    #[must_use]
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        let slot = match &mut self {
            Self::System(message) => &mut message.provider_options,
            Self::User(message) => &mut message.provider_options,
            Self::Assistant(message) => &mut message.provider_options,
            Self::Tool(message) => &mut message.provider_options,
        };
        *slot = Some(options);
        self
    }

    /// Returns the system message when this is one.
    #[must_use]
    pub fn as_system(&self) -> Option<&SystemMessage> {
        match self {
            Self::System(message) => Some(message),
            _ => None,
        }
    }

    /// Returns the user message when this is one.
    #[must_use]
    pub fn as_user(&self) -> Option<&UserMessage> {
        match self {
            Self::User(message) => Some(message),
            _ => None,
        }
    }

    /// Returns the assistant message when this is one.
    #[must_use]
    pub fn as_assistant(&self) -> Option<&AssistantMessage> {
        match self {
            Self::Assistant(message) => Some(message),
            _ => None,
        }
    }

    /// Returns the tool message when this is one.
    #[must_use]
    pub fn as_tool(&self) -> Option<&ToolMessage> {
        match self {
            Self::Tool(message) => Some(message),
            _ => None,
        }
    }
}

impl From<SystemMessage> for Message {
    fn from(message: SystemMessage) -> Self {
        Self::System(message)
    }
}

impl From<UserMessage> for Message {
    fn from(message: UserMessage) -> Self {
        Self::User(message)
    }
}

impl From<AssistantMessage> for Message {
    fn from(message: AssistantMessage) -> Self {
        Self::Assistant(message)
    }
}

impl From<ToolMessage> for Message {
    fn from(message: ToolMessage) -> Self {
        Self::Tool(message)
    }
}

/// Helpers for message histories.
pub trait MessagesExt {
    /// Appends an approval response to the trailing tool message, creating
    /// one when the history does not end with a tool message.
    fn push_approval_response(&mut self, response: ToolApprovalResponse);

    /// Returns the approval requests that have no response yet.
    fn pending_approval_requests(&self) -> Vec<&crate::part::ToolApprovalRequest>;
}

impl MessagesExt for Vec<Message> {
    fn push_approval_response(&mut self, response: ToolApprovalResponse) {
        if let Some(Message::Tool(tool)) = self.last_mut() {
            tool.content.push(ToolPart::ToolApprovalResponse(response));
        } else {
            self.push(Message::tool([ToolPart::ToolApprovalResponse(response)]));
        }
    }

    fn pending_approval_requests(&self) -> Vec<&crate::part::ToolApprovalRequest> {
        let answered: std::collections::HashSet<&ferrin_spec::ApprovalId> = self
            .iter()
            .filter_map(Message::as_tool)
            .flat_map(|tool| tool.content.iter())
            .filter_map(ToolPart::as_tool_approval_response)
            .map(|response| &response.approval_id)
            .collect();
        self.iter()
            .filter_map(Message::as_assistant)
            .filter_map(|assistant| assistant.content.as_parts())
            .flatten()
            .filter_map(AssistantPart::as_tool_approval_request)
            .filter(|request| !answered.contains(&request.approval_id))
            .collect()
    }
}
