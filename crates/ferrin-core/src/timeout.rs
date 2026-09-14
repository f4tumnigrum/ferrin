//! Timeout configuration.

use std::collections::HashMap;
use std::time::Duration;

use ferrin_spec::ToolName;
use serde::Deserialize;
use serde::Serialize;

/// Timeouts applied to a call.
///
/// `first_chunk` and `chunk` only apply to streaming calls; `total` covers
/// the whole call including tool execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeout {
    /// Whole call.
    pub total: Option<Duration>,
    /// One step (model call plus its tool executions).
    pub step: Option<Duration>,
    /// Streaming: from the request until the first content part.
    pub first_chunk: Option<Duration>,
    /// Streaming: between two consecutive content parts.
    pub chunk: Option<Duration>,
    /// Default per tool execution.
    pub tool: Option<Duration>,
    /// Per-tool overrides of `tool`.
    pub per_tool: HashMap<ToolName, Duration>,
}

impl Timeout {
    /// No timeouts.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Sets the total timeout.
    #[must_use]
    pub fn with_total(mut self, total: Duration) -> Self {
        self.total = Some(total);
        self
    }

    /// Sets the step timeout.
    #[must_use]
    pub fn with_step(mut self, step: Duration) -> Self {
        self.step = Some(step);
        self
    }

    /// Sets the first-chunk timeout (streaming only).
    #[must_use]
    pub fn with_first_chunk(mut self, first_chunk: Duration) -> Self {
        self.first_chunk = Some(first_chunk);
        self
    }

    /// Sets the inter-chunk timeout (streaming only).
    #[must_use]
    pub fn with_chunk(mut self, chunk: Duration) -> Self {
        self.chunk = Some(chunk);
        self
    }

    /// Sets the default tool timeout.
    #[must_use]
    pub fn with_tool(mut self, tool: Duration) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Sets the timeout of one tool.
    #[must_use]
    pub fn with_tool_for(mut self, name: impl Into<ToolName>, timeout: Duration) -> Self {
        self.per_tool.insert(name.into(), timeout);
        self
    }

    /// Returns the timeout for `tool`: the per-tool override, else the
    /// default tool timeout.
    #[must_use]
    pub fn tool_timeout(&self, tool: &ToolName) -> Option<Duration> {
        self.per_tool.get(tool).copied().or(self.tool)
    }
}

impl From<Duration> for Timeout {
    /// A bare duration is the total timeout.
    fn from(total: Duration) -> Self {
        Self::default().with_total(total)
    }
}

/// Which timeout fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TimeoutScope {
    /// [`Timeout::total`].
    Total,
    /// [`Timeout::step`].
    Step,
    /// [`Timeout::first_chunk`].
    FirstChunk,
    /// [`Timeout::chunk`].
    Chunk,
    /// [`Timeout::tool`] or a per-tool override.
    Tool {
        /// The tool that timed out.
        tool_name: ToolName,
    },
}

impl std::fmt::Display for TimeoutScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Total => f.write_str("total"),
            Self::Step => f.write_str("step"),
            Self::FirstChunk => f.write_str("first chunk"),
            Self::Chunk => f.write_str("chunk"),
            Self::Tool { tool_name } => write!(f, "tool `{tool_name}`"),
        }
    }
}
