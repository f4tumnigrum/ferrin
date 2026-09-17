//! Standardization of builder inputs into system instructions plus messages.

use ferrin_message::Message;
use ferrin_message::Role;
use ferrin_message::SystemMessage;
use ferrin_spec::ProviderOptions;

use crate::error::Error;

/// One system message or an ordered sequence of system messages.
///
/// String conversions create a single message. Use [`Self::messages`] for
/// several instruction messages with independent provider options.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Instructions {
    /// A single system message.
    System(SystemMessage),
    /// System messages in prompt order (an empty vector adds none).
    Messages(Vec<SystemMessage>),
}

impl Default for Instructions {
    fn default() -> Self {
        Self::new("")
    }
}

impl Instructions {
    /// Creates a single system instruction from text.
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self::System(SystemMessage {
            content: content.into(),
            provider_options: None,
        })
    }

    /// Creates ordered instructions with independent message metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferrin_core::Instructions;
    /// use ferrin_message::SystemMessage;
    /// let instructions = Instructions::messages([
    ///     SystemMessage { content: "Be concise.".into(), provider_options: None },
    ///     SystemMessage { content: "Use supplied sources.".into(), provider_options: None },
    /// ]);
    /// assert_eq!(instructions.as_messages().len(), 2);
    /// ```
    #[must_use]
    pub fn messages(messages: impl IntoIterator<Item = SystemMessage>) -> Self {
        Self::Messages(messages.into_iter().collect())
    }

    /// Replaces provider options on every configured system message.
    #[must_use]
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        match &mut self {
            Self::System(message) => message.provider_options = Some(options),
            Self::Messages(messages) => {
                for message in messages {
                    message.provider_options = Some(options.clone());
                }
            }
        }
        self
    }

    /// Borrows instruction messages in prompt order.
    #[must_use]
    pub fn as_messages(&self) -> &[SystemMessage] {
        match self {
            Self::System(message) => std::slice::from_ref(message),
            Self::Messages(messages) => messages,
        }
    }

    /// Consumes the instructions into ordered system messages.
    #[must_use]
    pub fn into_messages(self) -> Vec<SystemMessage> {
        match self {
            Self::System(message) => vec![message],
            Self::Messages(messages) => messages,
        }
    }
}

impl From<&str> for Instructions {
    fn from(content: &str) -> Self {
        Self::new(content)
    }
}

impl From<String> for Instructions {
    fn from(content: String) -> Self {
        Self::new(content)
    }
}

impl From<SystemMessage> for Instructions {
    fn from(message: SystemMessage) -> Self {
        Self::System(message)
    }
}

impl From<Vec<SystemMessage>> for Instructions {
    fn from(messages: Vec<SystemMessage>) -> Self {
        Self::Messages(messages)
    }
}

/// The standardized prompt: optional system instructions and non-empty
/// messages without system role (unless explicitly allowed).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StandardizedPrompt {
    pub(crate) system: Option<Instructions>,
    pub(crate) messages: Vec<Message>,
}

/// Standardizes builder inputs.
///
/// Exactly one of `prompt` and `messages` must be set; `messages` must not be
/// empty and must not contain system messages unless
/// `allow_system_in_messages` is set.
pub(crate) fn standardize(
    system: Option<Instructions>,
    prompt: Option<String>,
    messages: Option<Vec<Message>>,
    allow_system_in_messages: bool,
) -> Result<StandardizedPrompt, Error> {
    let messages = match (prompt, messages) {
        (Some(_), Some(_)) => {
            return Err(Error::invalid_prompt(
                "prompt and messages cannot be set at the same time",
            ));
        }
        (None, None) => {
            return Err(Error::invalid_prompt(
                "either prompt or messages must be set",
            ));
        }
        (Some(text), None) => vec![Message::user(text)],
        (None, Some(messages)) => messages,
    };
    if messages.is_empty() {
        return Err(Error::invalid_prompt("messages must not be empty"));
    }
    if !allow_system_in_messages
        && let Some(index) = messages
            .iter()
            .position(|message| message.role() == Role::System)
    {
        return Err(Error::invalid_prompt(format!(
            "messages must not contain system messages (found at index {index}); \
             use `system` or enable `allow_system_in_messages`"
        )));
    }
    Ok(StandardizedPrompt { system, messages })
}
