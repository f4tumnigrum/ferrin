//! Argument, prompt and configuration errors.

use super::BoxError;
use crate::json::JsonValue;

/// A call argument is invalid.
#[derive(Debug, thiserror::Error)]
#[error("invalid argument `{argument}`: {message}")]
pub struct InvalidArgumentError {
    /// Name of the argument.
    pub argument: String,
    /// What is wrong with it.
    pub message: String,
    /// Underlying cause.
    #[source]
    pub cause: Option<BoxError>,
}

impl InvalidArgumentError {
    /// Creates an error for `argument`.
    #[must_use]
    pub fn new(argument: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            argument: argument.into(),
            message: message.into(),
            cause: None,
        }
    }

    /// Sets the underlying cause.
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

/// The prompt cannot be used with this provider.
#[derive(Debug, thiserror::Error)]
#[error("invalid prompt: {message}")]
pub struct InvalidPromptError {
    /// What is wrong with the prompt.
    pub message: String,
    /// The offending prompt (or part of it) as JSON, if available.
    pub prompt: Option<JsonValue>,
    /// Underlying cause.
    #[source]
    pub cause: Option<BoxError>,
}

impl InvalidPromptError {
    /// Creates an error with a message only.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            prompt: None,
            cause: None,
        }
    }

    /// Attaches the offending prompt.
    #[must_use]
    pub fn with_prompt(mut self, prompt: JsonValue) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Sets the underlying cause.
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

/// The API key could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct LoadApiKeyError {
    /// Explanation (never contains the key).
    pub message: String,
}

impl LoadApiKeyError {
    /// Creates an error with `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A required setting could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct LoadSettingError {
    /// Explanation.
    pub message: String,
}

impl LoadSettingError {
    /// Creates an error with `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
