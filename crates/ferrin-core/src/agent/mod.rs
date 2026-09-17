//! Agents: reusable configurations of the generation loop.
//!
//! [`Agent`] is the abstraction; [`ToolLoopAgent`] is the built-in
//! implementation that delegates to `generate_text` and `stream_text`.
//! Applications may implement [`Agent`] themselves to combine models or
//! add planning logic; an agent can also serve as the executor of a tool
//! (sub-agent pattern), typically returning `GenerateTextResult::text()`
//! as the tool output.

mod options;
mod prepare_call;
mod tool_loop_agent;

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_tool::ToolSet;
use tokio_util::sync::CancellationToken;

pub use prepare_call::PrepareCall;
pub use prepare_call::PrepareCallInput;
pub use prepare_call::PreparedCall;
pub use tool_loop_agent::AGENT_USER_AGENT;
pub use tool_loop_agent::ToolLoopAgent;
pub use tool_loop_agent::ToolLoopAgentBuilder;

use crate::error::Error;
use crate::generate_text::GenerateTextResult;
use crate::generate_text::StepResult;
use crate::hooks::HookFn;
use crate::hooks::Hooks;
use crate::stream_text::OnErrorFn;
use crate::stream_text::StreamConfig;
use crate::stream_text::StreamEvent;
use crate::stream_text::StreamTextResult;
use crate::stream_text::StreamTransform;
use crate::telemetry::AbortEvent;
use crate::telemetry::EndEvent;
use crate::timeout::Timeout;

/// An agent: a model plus configuration that answers prompts, possibly over
/// several tool-calling steps.
pub trait Agent: Send + Sync {
    /// Per-call options (`()` when the agent takes none).
    type Options: Send + 'static;
    /// Structured output type (`()` when there is none).
    type Output: Send + 'static;

    /// Optional identifier used in telemetry.
    fn id(&self) -> Option<&str>;

    /// Tools available to the agent.
    fn tools(&self) -> &ToolSet;

    /// Runs the agent to completion.
    fn generate(
        &self,
        call: AgentCall<Self::Options>,
    ) -> impl Future<Output = Result<GenerateTextResult<Self::Output>, Error>> + Send;

    /// Runs the agent as a stream.
    fn stream(
        &self,
        call: AgentStreamCall<Self::Options>,
    ) -> impl Future<Output = Result<StreamTextResult<Self::Output>, Error>> + Send;
}

/// What the agent is asked about.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentInput {
    /// A single user prompt.
    Prompt(String),
    /// A conversation.
    Messages(Vec<Message>),
}

impl From<&str> for AgentInput {
    fn from(text: &str) -> Self {
        Self::Prompt(text.to_owned())
    }
}

impl From<String> for AgentInput {
    fn from(text: String) -> Self {
        Self::Prompt(text)
    }
}

impl From<Vec<Message>> for AgentInput {
    fn from(messages: Vec<Message>) -> Self {
        Self::Messages(messages)
    }
}

/// Parameters of one agent call.
pub struct AgentCall<O> {
    /// The input.
    pub input: AgentInput,
    /// Per-call options.
    pub options: O,
    /// Cancels the call.
    pub cancellation: CancellationToken,
    /// Overrides the agent's timeouts when set.
    pub timeout: Option<Timeout>,
    /// Hooks invoked after the agent's own hooks, then awaited concurrently.
    pub hooks: Hooks,
    /// Sandbox passed to tools.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<dyn ferrin_tool::Sandbox>>,
}

impl<O: fmt::Debug> fmt::Debug for AgentCall<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentCall")
            .field("input", &self.input)
            .field("options", &self.options)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl AgentCall<()> {
    /// A call without options.
    #[must_use]
    pub fn new(input: impl Into<AgentInput>) -> Self {
        Self {
            input: input.into(),
            options: (),
            cancellation: CancellationToken::new(),
            timeout: None,
            hooks: Hooks::default(),
            #[cfg(feature = "sandbox")]
            sandbox: None,
        }
    }

    /// A call with a single user prompt.
    #[must_use]
    pub fn prompt(text: impl Into<String>) -> Self {
        Self::new(AgentInput::Prompt(text.into()))
    }

    /// A call with a conversation.
    #[must_use]
    pub fn messages(messages: impl IntoIterator<Item = Message>) -> Self {
        Self::new(AgentInput::Messages(messages.into_iter().collect()))
    }
}

impl<O> AgentCall<O> {
    /// Sets the per-call options.
    #[must_use]
    pub fn options<P>(self, options: P) -> AgentCall<P> {
        AgentCall {
            input: self.input,
            options,
            cancellation: self.cancellation,
            timeout: self.timeout,
            hooks: self.hooks,
            #[cfg(feature = "sandbox")]
            sandbox: self.sandbox,
        }
    }

    /// Sets the cancellation token.
    #[must_use]
    pub fn cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation = token;
        self
    }

    /// Overrides the agent's timeouts.
    #[must_use]
    pub fn timeout(mut self, timeout: impl Into<Timeout>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }

    /// Adds hooks (invoked after the agent's hooks, awaited concurrently).
    #[must_use]
    pub fn hooks(mut self, hooks: Hooks) -> Self {
        self.hooks = self.hooks.merged(hooks);
        self
    }

    /// Adds a step-end hook.
    #[must_use]
    pub fn on_step_end(mut self, f: impl HookFn<StepResult>) -> Self {
        self.hooks.on_step_end.push(Arc::new(f));
        self
    }

    /// Adds an end hook.
    #[must_use]
    pub fn on_end(mut self, f: impl HookFn<EndEvent>) -> Self {
        self.hooks.on_end.push(Arc::new(f));
        self
    }

    /// Sets the sandbox passed to tools.
    #[cfg(feature = "sandbox")]
    #[must_use]
    pub fn sandbox(mut self, sandbox: Arc<dyn ferrin_tool::Sandbox>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Converts into a streaming call.
    #[must_use]
    pub fn streaming(self) -> AgentStreamCall<O> {
        AgentStreamCall::new(self)
    }
}

/// Parameters of one streaming agent call.
pub struct AgentStreamCall<O> {
    /// The shared call parameters.
    pub call: AgentCall<O>,
    pub(crate) stream: StreamConfig,
}

impl<O: fmt::Debug> fmt::Debug for AgentStreamCall<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentStreamCall")
            .field("call", &self.call)
            .field("stream", &self.stream)
            .finish()
    }
}

impl<O> From<AgentCall<O>> for AgentStreamCall<O> {
    fn from(call: AgentCall<O>) -> Self {
        Self::new(call)
    }
}

impl<O> AgentStreamCall<O> {
    /// Wraps a call.
    #[must_use]
    pub fn new(call: AgentCall<O>) -> Self {
        Self {
            call,
            stream: StreamConfig::default(),
        }
    }

    /// Adds a stream transform (applied in order).
    #[must_use]
    pub fn transform(mut self, transform: impl StreamTransform + 'static) -> Self {
        self.stream.transforms.push(Arc::new(transform));
        self
    }

    /// Forwards raw provider chunks.
    #[must_use]
    pub fn include_raw_chunks(mut self) -> Self {
        self.stream.include_raw_chunks = true;
        self
    }

    /// Enables automatic retries of failed model streams.
    #[must_use]
    pub fn stream_retries(mut self, retries: u32) -> Self {
        self.stream.stream_retries = Some(retries);
        self
    }

    /// Sets the stream error callback.
    #[must_use]
    pub fn on_error(mut self, f: impl OnErrorFn) -> Self {
        self.stream.on_error = Some(Arc::new(f));
        self
    }

    /// Adds a chunk hook.
    #[must_use]
    pub fn on_chunk(mut self, f: impl HookFn<StreamEvent>) -> Self {
        self.call.hooks.on_chunk.push(Arc::new(f));
        self
    }

    /// Adds an abort hook.
    #[must_use]
    pub fn on_abort(mut self, f: impl HookFn<AbortEvent>) -> Self {
        self.call.hooks.on_abort.push(Arc::new(f));
        self
    }
}
