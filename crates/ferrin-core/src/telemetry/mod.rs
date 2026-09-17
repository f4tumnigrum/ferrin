//! Telemetry: lifecycle callbacks, options and the built-in `tracing` spans.
//!
//! Lifecycle callbacks are awaited concurrently and isolate unwinding panics.
//! The two `execute_*` wrappers preserve the actual call result.
//! Integrations are injected per call through
//! [`TelemetryOptions::integrations`]; there is no global registry.
//! Callback settlement and wrapper ordering follow the Vercel AI SDK
//! (`packages/ai/src/telemetry/create-telemetry-dispatcher.ts`); see `NOTICE`.

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
    /// Whether an integration may record the tool's output.
    pub record_outputs: bool,
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

macro_rules! callback {
    ($name:ident, $event:ty, $doc:literal) => {
        #[doc = $doc]
        fn $name<'a>(&'a self, _event: &'a $event) -> BoxFuture<'a, ()> {
            Box::pin(async {})
        }
    };
}

/// A telemetry integration with awaited lifecycle callbacks.
///
/// Every method has a no-op default. Integrations for the same event run
/// concurrently; unwinding callback panics cannot interrupt the operation.
pub trait Telemetry: Send + Sync + 'static {
    callback!(on_start, StartEvent, "A call started.");
    callback!(on_step_start, StepStartEvent, "A step started.");
    callback!(
        on_language_model_call_start,
        ModelCallStartEvent,
        "A model call is about to be made."
    );
    callback!(
        on_language_model_call_end,
        ModelCallEndEvent,
        "A model call finished."
    );
    callback!(
        on_tool_execution_start,
        ToolExecutionStartEvent,
        "A tool execution started."
    );
    callback!(
        on_tool_execution_end,
        ToolExecutionEndEvent,
        "A tool execution finished."
    );
    callback!(on_step_end, StepEndEvent, "A step finished.");
    callback!(
        on_embed_start,
        EmbedStartEvent,
        "An embedding call started."
    );
    callback!(on_embed_end, EmbedEndEvent, "An embedding call finished.");
    callback!(on_rerank_start, RerankStartEvent, "A rerank call started.");
    callback!(on_rerank_end, RerankEndEvent, "A rerank call finished.");
    callback!(
        on_embed_operation_start,
        crate::embed::EmbedCallStartEvent,
        "An embedding operation started before its attempts."
    );
    callback!(
        on_embed_operation_end,
        crate::embed::EmbedCallEndEvent,
        "An embedding operation completed all its attempts."
    );
    callback!(
        on_rerank_operation_start,
        crate::rerank::RerankCallStartEvent,
        "A reranking operation started before its attempts."
    );
    callback!(
        on_rerank_operation_end,
        crate::rerank::RerankCallEndEvent,
        "A reranking operation completed all its attempts."
    );
    callback!(on_end, EndEvent, "A call finished.");
    callback!(on_abort, AbortEvent, "A streaming call was aborted.");
    callback!(on_error, ErrorEvent<'_>, "An error occurred.");

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
    /// Whether application runtime context is included in telemetry events and steps.
    pub include_runtime_context: bool,
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
            .field("include_runtime_context", &self.include_runtime_context)
            .field("include_tools_context", &self.include_tools_context)
            .field("integrations", &self.integrations.len())
            .finish()
    }
}
