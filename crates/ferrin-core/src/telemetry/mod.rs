//! Telemetry: lifecycle callbacks, options and the built-in `tracing` spans.
//!
//! Callbacks are synchronous (they run on the pipeline hot path); the two
//! `execute_*` wrappers are asynchronous because they must wrap the actual
//! call. Integrations are injected per call through
//! [`TelemetryOptions::integrations`]; there is no global registry.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamResult;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_tool::ToolError;

use crate::error::Error;

pub(crate) mod dispatcher;
mod events;
mod redact;
mod redact_provider;
pub(crate) mod spans;

pub(crate) use dispatcher::TelemetryDispatcher;
pub use events::AbortEvent;
pub use events::EmbedEndEvent;
pub use events::EmbedStartEvent;
pub use events::EndEvent;
pub use events::ErrorEvent;
pub use events::ErrorPhase;
pub use events::ModelCallEndEvent;
pub use events::ModelCallStartEvent;
pub use events::ModelIdentity;
pub use events::RecordedInputs;
pub use events::RerankEndEvent;
pub use events::RerankStartEvent;
pub use events::StartEvent;
pub use events::StepEndEvent;
pub use events::StepStartEvent;
pub use events::ToolExecutionEndEvent;
pub use events::ToolExecutionStartEvent;
pub use events::ToolOutcome;
pub use spans::WARNINGS_TARGET;

/// Context of a model call handed to [`Telemetry::execute_language_model_call`].
#[derive(Debug, Clone)]
pub struct ModelCallContext {
    /// Call id.
    pub call_id: String,
    /// Zero-based step index.
    pub step_number: u32,
    /// The model.
    pub model: ModelIdentity,
    /// Function id from the telemetry options.
    pub function_id: Option<String>,
}

/// Context of a tool execution handed to [`Telemetry::execute_tool`].
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// Call id.
    pub call_id: String,
    /// Tool call id.
    pub tool_call_id: ToolCallId,
    /// Tool name.
    pub tool_name: ToolName,
    /// Input (only when `record_inputs`).
    pub input: Option<JsonValue>,
}

/// Result of a model call as seen by [`Telemetry::execute_language_model_call`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelCallOutcome {
    /// A non-streaming result.
    Generate(Box<GenerateResult>),
    /// A streaming result (the stream has not been consumed yet).
    Stream(Box<StreamResult>),
}

/// A telemetry integration.
///
/// Every method has a no-op default; implement the ones you need. Callbacks
/// must not block: hand events to your own channel when processing is slow.
pub trait Telemetry: Send + Sync + 'static {
    /// A call started.
    fn on_start(&self, _event: &StartEvent) {}
    /// A step started.
    fn on_step_start(&self, _event: &StepStartEvent) {}
    /// A model call is about to be made.
    fn on_language_model_call_start(&self, _event: &ModelCallStartEvent) {}
    /// A model call finished.
    fn on_language_model_call_end(&self, _event: &ModelCallEndEvent) {}
    /// A tool execution started.
    fn on_tool_execution_start(&self, _event: &ToolExecutionStartEvent) {}
    /// A tool execution finished.
    fn on_tool_execution_end(&self, _event: &ToolExecutionEndEvent) {}
    /// A step finished.
    fn on_step_end(&self, _event: &StepEndEvent) {}
    /// An embedding call started.
    fn on_embed_start(&self, _event: &EmbedStartEvent) {}
    /// An embedding call finished.
    fn on_embed_end(&self, _event: &EmbedEndEvent) {}
    /// A rerank call started.
    fn on_rerank_start(&self, _event: &RerankStartEvent) {}
    /// A rerank call finished.
    fn on_rerank_end(&self, _event: &RerankEndEvent) {}
    /// A call finished.
    fn on_end(&self, _event: &EndEvent) {}
    /// A streaming call was aborted.
    fn on_abort(&self, _event: &AbortEvent) {}
    /// An error occurred.
    fn on_error(&self, _event: &ErrorEvent<'_>) {}

    /// Runs a model call inside integration-specific context (for example an
    /// OpenTelemetry span). The default runs `call` unchanged.
    fn execute_language_model_call<'a>(
        &'a self,
        _ctx: &'a ModelCallContext,
        call: BoxFuture<'a, Result<ModelCallOutcome, Error>>,
    ) -> BoxFuture<'a, Result<ModelCallOutcome, Error>> {
        call
    }

    /// Runs a tool execution inside integration-specific context.
    fn execute_tool<'a>(
        &'a self,
        _ctx: &'a ToolExecutionContext,
        call: BoxFuture<'a, Result<ToolOutcome, ToolError>>,
    ) -> BoxFuture<'a, Result<ToolOutcome, ToolError>> {
        call
    }
}

/// Telemetry configuration of a call.
#[derive(Clone, Default)]
pub struct TelemetryOptions {
    /// Whether telemetry callbacks fire at all.
    pub enabled: bool,
    /// Whether prompts, call options and tool inputs are included in events.
    pub record_inputs: bool,
    /// Whether generated content and tool outputs are included in events.
    pub record_outputs: bool,
    /// Identifier of the calling function for grouping.
    pub function_id: Option<String>,
    /// Free-form metadata attached to the start event.
    pub metadata: BTreeMap<String, JsonValue>,
    /// Whether the tools context is attached to tool events.
    pub include_tools_context: bool,
    /// Integrations that receive the events.
    pub integrations: Vec<Arc<dyn Telemetry>>,
}

impl TelemetryOptions {
    /// Enabled options recording neither inputs nor outputs.
    #[must_use]
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// Adds an integration.
    #[must_use]
    pub fn with_integration(mut self, integration: Arc<dyn Telemetry>) -> Self {
        self.integrations.push(integration);
        self
    }
}

impl fmt::Debug for TelemetryOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryOptions")
            .field("enabled", &self.enabled)
            .field("record_inputs", &self.record_inputs)
            .field("record_outputs", &self.record_outputs)
            .field("function_id", &self.function_id)
            .field("metadata", &self.metadata)
            .field("include_tools_context", &self.include_tools_context)
            .field("integrations", &self.integrations.len())
            .finish()
    }
}
