//! Finish reasons.

use serde::Deserialize;
use serde::Serialize;

/// Unified reason why a language model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FinishReasonKind {
    /// The model generated a stop sequence or finished naturally.
    Stop,
    /// The output token limit was reached.
    Length,
    /// Content was filtered by the provider.
    ContentFilter,
    /// The model requested tool calls.
    ToolCalls,
    /// The provider reported an error.
    Error,
    /// Any other reason; see `raw`.
    Other,
}

/// Finish reason with the provider's raw value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReason {
    /// The unified reason.
    pub unified: FinishReasonKind,
    /// The provider's original finish reason string, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl FinishReason {
    /// Creates a finish reason without a raw value.
    #[must_use]
    pub fn new(unified: FinishReasonKind) -> Self {
        Self { unified, raw: None }
    }

    /// Creates a finish reason with the provider's raw value.
    #[must_use]
    pub fn with_raw(unified: FinishReasonKind, raw: impl Into<String>) -> Self {
        Self {
            unified,
            raw: Some(raw.into()),
        }
    }

    /// Shorthand for [`FinishReasonKind::Stop`] without a raw value.
    #[must_use]
    pub fn stop() -> Self {
        Self::new(FinishReasonKind::Stop)
    }

    /// Shorthand for [`FinishReasonKind::ToolCalls`] without a raw value.
    #[must_use]
    pub fn tool_calls() -> Self {
        Self::new(FinishReasonKind::ToolCalls)
    }

    /// Shorthand for [`FinishReasonKind::Error`] without a raw value.
    #[must_use]
    pub fn error() -> Self {
        Self::new(FinishReasonKind::Error)
    }
}

impl From<FinishReasonKind> for FinishReason {
    fn from(unified: FinishReasonKind) -> Self {
        Self::new(unified)
    }
}

impl std::fmt::Display for FinishReasonKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ContentFilter => "content-filter",
            Self::ToolCalls => "tool-calls",
            Self::Error => "error",
            Self::Other => "other",
        };
        f.write_str(text)
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.raw {
            Some(raw) if raw != &self.unified.to_string() => {
                write!(f, "{} ({raw})", self.unified)
            }
            _ => write!(f, "{}", self.unified),
        }
    }
}
