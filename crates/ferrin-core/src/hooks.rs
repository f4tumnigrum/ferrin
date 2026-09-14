//! Lifecycle hooks: asynchronous callbacks awaited by the core.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use ferrin_spec::BoxFuture;

use crate::generate_text::StepResult;
use crate::stream_text::StreamEvent;
use crate::telemetry::AbortEvent;
use crate::telemetry::EndEvent;
use crate::telemetry::ModelCallEndEvent;
use crate::telemetry::ModelCallStartEvent;
use crate::telemetry::StartEvent;
use crate::telemetry::StepStartEvent;
use crate::telemetry::ToolExecutionEndEvent;
use crate::telemetry::ToolExecutionStartEvent;

/// An asynchronous callback for event type `E`.
///
/// Implemented for every `Fn(Arc<E>) -> impl Future<Output = ()>` closure, so
/// builders accept `|event| async move { ... }` directly. The core awaits the
/// returned future before continuing.
pub trait HookFn<E>: Send + Sync + 'static {
    /// Handles one event.
    fn call(&self, event: Arc<E>) -> BoxFuture<'static, ()>;
}

impl<E, F, Fut> HookFn<E> for F
where
    F: Fn(Arc<E>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn call(&self, event: Arc<E>) -> BoxFuture<'static, ()> {
        Box::pin(self(event))
    }
}

/// A list of hooks for one event type.
pub type HookList<E> = Vec<Arc<dyn HookFn<E>>>;

/// All lifecycle hooks of a call.
#[derive(Clone, Default)]
pub struct Hooks {
    /// The call started.
    pub on_start: HookList<StartEvent>,
    /// A step started.
    pub on_step_start: HookList<StepStartEvent>,
    /// A model call is about to be made.
    pub on_language_model_call_start: HookList<ModelCallStartEvent>,
    /// A model call finished (tool calls parsed, not yet executed).
    pub on_language_model_call_end: HookList<ModelCallEndEvent>,
    /// A tool execution started.
    pub on_tool_execution_start: HookList<ToolExecutionStartEvent>,
    /// A tool execution finished.
    pub on_tool_execution_end: HookList<ToolExecutionEndEvent>,
    /// A step finished.
    pub on_step_end: HookList<StepResult>,
    /// The call finished.
    pub on_end: HookList<EndEvent>,
    /// Streaming: an event was emitted.
    pub on_chunk: HookList<StreamEvent>,
    /// Streaming: the call was aborted.
    pub on_abort: HookList<AbortEvent>,
}

impl Hooks {
    /// Appends the hooks of `other` after those of `self` (settings-level
    /// hooks run before call-level hooks).
    #[must_use]
    pub fn merged(mut self, other: Hooks) -> Hooks {
        self.on_start.extend(other.on_start);
        self.on_step_start.extend(other.on_step_start);
        self.on_language_model_call_start
            .extend(other.on_language_model_call_start);
        self.on_language_model_call_end
            .extend(other.on_language_model_call_end);
        self.on_tool_execution_start
            .extend(other.on_tool_execution_start);
        self.on_tool_execution_end
            .extend(other.on_tool_execution_end);
        self.on_step_end.extend(other.on_step_end);
        self.on_end.extend(other.on_end);
        self.on_chunk.extend(other.on_chunk);
        self.on_abort.extend(other.on_abort);
        self
    }

    /// Runs every hook of `list` in order with `event`.
    pub async fn emit<E: 'static>(list: &[Arc<dyn HookFn<E>>], event: Arc<E>) {
        for hook in list {
            hook.call(Arc::clone(&event)).await;
        }
    }
}

impl fmt::Debug for Hooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hooks")
            .field("on_start", &self.on_start.len())
            .field("on_step_start", &self.on_step_start.len())
            .field(
                "on_language_model_call_start",
                &self.on_language_model_call_start.len(),
            )
            .field(
                "on_language_model_call_end",
                &self.on_language_model_call_end.len(),
            )
            .field(
                "on_tool_execution_start",
                &self.on_tool_execution_start.len(),
            )
            .field("on_tool_execution_end", &self.on_tool_execution_end.len())
            .field("on_step_end", &self.on_step_end.len())
            .field("on_end", &self.on_end.len())
            .field("on_chunk", &self.on_chunk.len())
            .field("on_abort", &self.on_abort.len())
            .finish()
    }
}
