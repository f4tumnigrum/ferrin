//! Standardization of builder inputs into system instructions plus messages.

use ferrin_message::Message;
use ferrin_message::Role;
use ferrin_message::SystemMessage;
use ferrin_spec::ProviderOptions;

use crate::error::Error;

/// System instructions: text with optional provider options.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Instructions {
    /// The instruction text.
    pub content: String,
    /// Provider-specific options for the system message.
    pub provider_options: Option<ProviderOptions>,
}

impl Instructions {
    /// Creates instructions from text.
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            provider_options: None,
        }
    }

    /// Sets provider options.
    #[must_use]
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Converts into a system message.
    #[must_use]
    pub fn into_message(self) -> SystemMessage {
        SystemMessage {
            content: self.content,
            provider_options: self.provider_options,
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
        Self {
            content: message.content,
            provider_options: message.provider_options,
        }
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
