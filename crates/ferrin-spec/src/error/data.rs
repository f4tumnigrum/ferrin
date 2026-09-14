//! Response data, parsing and validation errors.

use super::BoxError;
use crate::json::JsonValue;

/// The provider returned an empty body where one was required.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct EmptyResponseBodyError {
    /// Explanation.
    pub message: String,
}

impl EmptyResponseBodyError {
    /// Creates an error with the default message.
    #[must_use]
    pub fn new() -> Self {
        Self {
            message: "empty response body".to_owned(),
        }
    }

    /// Creates an error with a custom message.
    #[must_use]
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Default for EmptyResponseBodyError {
    fn default() -> Self {
        Self::new()
    }
}

/// The provider produced no content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct NoContentGeneratedError {
    /// Explanation.
    pub message: String,
}

impl NoContentGeneratedError {
    /// Creates an error with the default message.
    #[must_use]
    pub fn new() -> Self {
        Self {
            message: "no content generated".to_owned(),
        }
    }

    /// Creates an error with a custom message.
    #[must_use]
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Default for NoContentGeneratedError {
    fn default() -> Self {
        Self::new()
    }
}

/// The response has an unexpected shape.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct InvalidResponseDataError {
    /// Explanation.
    pub message: String,
    /// The offending data.
    pub data: JsonValue,
}

impl InvalidResponseDataError {
    /// Creates an error with a message and the offending data.
    #[must_use]
    pub fn new(message: impl Into<String>, data: JsonValue) -> Self {
        Self {
            message: message.into(),
            data,
        }
    }

    /// Creates an error whose message is derived from the data.
    #[must_use]
    pub fn from_data(data: JsonValue) -> Self {
        let rendered = data.to_string();
        let message = format!(
            "invalid response data: {}",
            super::truncate_for_display(&rendered, 512)
        );
        Self { message, data }
    }
}

/// Text could not be parsed as JSON.
#[derive(Debug, thiserror::Error)]
#[error("json parsing failed: {cause}")]
pub struct JsonParseError {
    /// The text that failed to parse.
    pub text: String,
    /// The parser error.
    #[source]
    pub cause: BoxError,
}

impl JsonParseError {
    /// Creates an error for `text` caused by `cause`.
    #[must_use]
    pub fn new(
        text: impl Into<String>,
        cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            text: text.into(),
            cause: Box::new(cause),
        }
    }
}

/// Where a validated value came from, for error messages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeValidationContext {
    /// Field or path being validated.
    pub field: Option<String>,
    /// Kind of entity (for example `tool input`).
    pub entity_name: Option<String>,
    /// Identifier of the entity (for example a tool call id).
    pub entity_id: Option<String>,
}

/// A value failed schema or type validation.
#[derive(Debug, thiserror::Error)]
pub struct TypeValidationError {
    /// The offending value.
    pub value: JsonValue,
    /// Where the value came from (boxed to keep the error small).
    pub context: Option<Box<TypeValidationContext>>,
    /// The validation error.
    #[source]
    pub cause: BoxError,
}

impl TypeValidationError {
    /// Creates an error for `value` caused by `cause`.
    #[must_use]
    pub fn new(value: JsonValue, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            value,
            context: None,
            cause: Box::new(cause),
        }
    }

    /// Sets the validation context.
    #[must_use]
    pub fn with_context(mut self, context: TypeValidationContext) -> Self {
        self.context = Some(Box::new(context));
        self
    }

    /// Wraps `cause` unless it already is a [`TypeValidationError`] for the
    /// same value and context, in which case it is returned unchanged.
    #[must_use]
    pub fn wrap(value: JsonValue, cause: BoxError, context: Option<TypeValidationContext>) -> Self {
        let context = context.map(Box::new);
        match cause.downcast::<TypeValidationError>() {
            Ok(existing) if existing.value == value && existing.context == context => *existing,
            Ok(existing) => Self {
                value,
                context,
                cause: existing,
            },
            Err(cause) => Self {
                value,
                context,
                cause,
            },
        }
    }
}

impl std::fmt::Display for TypeValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("type validation failed")?;
        if let Some(context) = &self.context {
            if let Some(field) = &context.field {
                write!(f, " for {field}")?;
            }
            let mut parts = Vec::new();
            if let Some(name) = &context.entity_name {
                parts.push(name.clone());
            }
            if let Some(id) = &context.entity_id {
                parts.push(format!("id: \"{id}\""));
            }
            if !parts.is_empty() {
                write!(f, " ({})", parts.join(", "))?;
            }
        }
        write!(f, ": {}", self.cause)
    }
}
