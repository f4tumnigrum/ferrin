//! Tool-level errors.

use std::time::Duration;

use ferrin_spec::JsonValue;
use ferrin_spec::ToolName;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Error returned by a tool execution.
///
/// Tool errors are not fatal: the core turns them into `error-text` /
/// `error-json` tool results that are fed back to the model. Only
/// [`ToolError::Cancelled`] aborts the whole call.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// A textual error.
    #[error("{message}")]
    Message {
        /// Explanation shown to the model.
        message: String,
        /// Underlying error.
        #[source]
        cause: Option<BoxError>,
    },
    /// A structured error payload.
    #[error("tool returned error payload")]
    Json {
        /// The payload.
        value: JsonValue,
    },
    /// The execution exceeded its timeout.
    #[error("tool execution timed out after {0:?}")]
    Timeout(Duration),
    /// The execution was cancelled.
    #[error("tool execution cancelled")]
    Cancelled,
}

impl ToolError {
    /// A textual error.
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
            cause: None,
        }
    }

    /// A structured error.
    #[must_use]
    pub fn json(value: JsonValue) -> Self {
        Self::Json { value }
    }

    /// Wraps any error, using its display text as the message.
    #[must_use]
    pub fn from_error(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Message {
            message: error.to_string(),
            cause: Some(Box::new(error)),
        }
    }

    /// Attaches a cause to a [`ToolError::Message`]; other variants are
    /// returned unchanged.
    #[must_use]
    pub fn with_cause(self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        match self {
            Self::Message { message, .. } => Self::Message {
                message,
                cause: Some(Box::new(cause)),
            },
            other => other,
        }
    }

    /// Returns `true` for [`ToolError::Cancelled`].
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(error: serde_json::Error) -> Self {
        Self::from_error(error)
    }
}

/// A tool name was inserted twice into a [`crate::ToolSet`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("tool `{name}` is already defined")]
pub struct DuplicateToolError {
    /// The duplicated name.
    pub name: ToolName,
}
