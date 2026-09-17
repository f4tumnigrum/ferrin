//! Execution contract: [`ToolExecute`], [`ToolOutput`], [`ToolContext`].

use std::fmt;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_spec::BoxStream;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::error::ToolError;

/// Stream of outputs produced by one execution.
pub type ToolOutputStream = BoxStream<'static, Result<ToolOutput, ToolError>>;

/// One output of a tool execution.
///
/// Streaming tools emit any number of [`ToolOutput::Preliminary`] values
/// followed by exactly one [`ToolOutput::Final`]; single-value tools emit
/// only the final value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolOutput {
    /// An intermediate result (`preliminary: true` in the reference model).
    Preliminary(JsonValue),
    /// The final result.
    Final(JsonValue),
}

impl ToolOutput {
    /// Returns `true` for the final output.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Final(_))
    }

    /// The JSON value.
    #[must_use]
    pub fn value(&self) -> &JsonValue {
        match self {
            Self::Preliminary(value) | Self::Final(value) => value,
        }
    }

    /// Consumes the output, returning the JSON value.
    #[must_use]
    pub fn into_value(self) -> JsonValue {
        match self {
            Self::Preliminary(value) | Self::Final(value) => value,
        }
    }
}

/// Executes a tool.
///
/// Implement this for custom execution strategies (remote execution,
/// recording); ordinary tools use the closure adapters on [`crate::ToolBuilder`].
/// Implementations receive input that already passed the tool's input
/// schema and must honour `ctx.cancellation`.
pub trait ToolExecute: Send + Sync {
    /// Starts an execution.
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> ToolOutputStream;
}

/// Per-call information handed to a tool execution.
#[derive(Clone)]
pub struct ToolContext {
    /// Id of the tool call being executed.
    pub tool_call_id: ToolCallId,
    /// Messages sent to the model for the step that produced the call
    /// (without the system prompt and without the assistant response).
    pub messages: Arc<[Message]>,
    /// Cancels the execution.
    pub cancellation: CancellationToken,
    /// This tool's selected context, validated when it declares a context schema.
    pub tools_context: Option<JsonValue>,
    /// Sandbox the tool operates in.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<dyn crate::sandbox::Sandbox>>,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ToolContext");
        debug
            .field("tool_call_id", &self.tool_call_id)
            .field("messages", &self.messages.len())
            .field("cancelled", &self.cancellation.is_cancelled())
            .field("tools_context", &self.tools_context);
        #[cfg(feature = "sandbox")]
        debug.field(
            "sandbox",
            &self.sandbox.as_ref().map(|sandbox| sandbox.description()),
        );
        debug.finish()
    }
}

impl ToolContext {
    /// Creates a context with no messages, a fresh token and no tool context.
    #[must_use]
    pub fn new(tool_call_id: impl Into<ToolCallId>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages: Arc::from(Vec::new()),
            cancellation: CancellationToken::new(),
            tools_context: None,
            #[cfg(feature = "sandbox")]
            sandbox: None,
        }
    }

    /// Sets the step messages.
    #[must_use]
    pub fn with_messages(mut self, messages: impl Into<Arc<[Message]>>) -> Self {
        self.messages = messages.into();
        self
    }

    /// Sets the cancellation token.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Sets the validated tool context.
    #[must_use]
    pub fn with_tools_context(mut self, tools_context: Option<JsonValue>) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets the sandbox.
    #[cfg(feature = "sandbox")]
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: Arc<dyn crate::sandbox::Sandbox>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }
}

/// Drives an output stream to its end, forwarding preliminary values to
/// `on_preliminary` and returning the final value.
///
/// # Errors
///
/// Returns the first error produced by the stream, or a
/// [`ToolError::Message`] when the stream ends without a final output.
pub async fn execute_to_completion(
    mut stream: ToolOutputStream,
    mut on_preliminary: impl FnMut(JsonValue),
) -> Result<JsonValue, ToolError> {
    while let Some(item) = stream.next().await {
        match item? {
            ToolOutput::Preliminary(value) => on_preliminary(value),
            ToolOutput::Final(value) => return Ok(value),
        }
    }
    Err(ToolError::message("tool produced no final output"))
}
