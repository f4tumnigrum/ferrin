//! Fans telemetry events out to the configured integrations and to
//! `tracing`.

use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_tool::ToolError;

use super::AbortEvent;
use super::EmbedEndEvent;
use super::EmbedStartEvent;
use super::EndEvent;
use super::ErrorEvent;
use super::ModelCallContext;
use super::ModelCallEndEvent;
use super::ModelCallOutcome;
use super::ModelCallStartEvent;
use super::RerankEndEvent;
use super::RerankStartEvent;
use super::StartEvent;
use super::StepEndEvent;
use super::StepStartEvent;
use super::Telemetry;
use super::TelemetryOptions;
use super::ToolExecutionContext;
use super::ToolExecutionEndEvent;
use super::ToolExecutionStartEvent;
use super::ToolOutcome;
use crate::error::Error;

/// Dispatches events to every integration of a [`TelemetryOptions`].
#[derive(Clone, Debug)]
pub(crate) struct TelemetryDispatcher {
    options: Arc<TelemetryOptions>,
}

macro_rules! dispatch {
    ($name:ident, $event:ty) => {
        pub(crate) fn $name(&self, event: &$event) {
            if !self.options.enabled {
                return;
            }
            for integration in &self.options.integrations {
                integration.$name(event);
            }
        }
    };
}

impl TelemetryDispatcher {
    pub(crate) fn new(options: TelemetryOptions) -> Self {
        Self {
            options: Arc::new(options),
        }
    }

    pub(crate) fn record_inputs(&self) -> bool {
        self.options.enabled && self.options.record_inputs
    }

    pub(crate) fn record_outputs(&self) -> bool {
        self.options.enabled && self.options.record_outputs
    }

    dispatch!(on_start, StartEvent);
    dispatch!(on_step_start, StepStartEvent);
    dispatch!(on_language_model_call_start, ModelCallStartEvent);

    dispatch!(on_tool_execution_start, ToolExecutionStartEvent);
    pub(crate) fn on_tool_execution_end(&self, event: &ToolExecutionEndEvent) {
        if !self.options.enabled {
            return;
        }
        let mut recorded = event.clone();
        if !self.record_outputs() {
            recorded.output = None;
            recorded.error = recorded
                .error
                .map(|_| crate::generate_text::ToolErrorInfo::text(super::redact::REDACTED));
        }
        for integration in &self.options.integrations {
            integration.on_tool_execution_end(&recorded);
        }
    }

    dispatch!(on_abort, AbortEvent);

    pub(crate) fn on_language_model_call_end(&self, event: &ModelCallEndEvent) {
        if !self.options.enabled {
            return;
        }
        let mut recorded = event.clone();
        if !(self.record_inputs() && self.record_outputs()) {
            recorded.warnings = super::redact::warnings(&recorded.warnings);
        }
        if !self.record_outputs() {
            recorded.content = None;
            recorded.response.body = None;
        }
        for integration in &self.options.integrations {
            integration.on_language_model_call_end(&recorded);
        }
    }

    pub(crate) fn on_step_end(&self, event: &StepEndEvent) {
        if !self.options.enabled {
            return;
        }
        let recorded = StepEndEvent {
            call_id: event.call_id.clone(),
            step: Arc::new(self.recorded_step(&event.step)),
        };
        for integration in &self.options.integrations {
            integration.on_step_end(&recorded);
        }
    }

    pub(crate) fn on_end(&self, event: &EndEvent) {
        if !self.options.enabled {
            return;
        }
        let recorded = EndEvent {
            call_id: event.call_id.clone(),
            steps: event
                .steps
                .iter()
                .map(|step| self.recorded_step(step))
                .collect(),
            total_usage: event.total_usage.clone(),
            output_recorded: self
                .record_outputs()
                .then(|| event.output_recorded.clone())
                .flatten(),
        };
        for integration in &self.options.integrations {
            integration.on_end(&recorded);
        }
    }

    /// Filters the telemetry copy without changing application hooks or results.
    fn recorded_step(
        &self,
        step: &crate::generate_text::StepResult,
    ) -> crate::generate_text::StepResult {
        let mut recorded = step.clone();
        if !(self.record_inputs() && self.record_outputs()) {
            recorded.warnings = super::redact::warnings(&recorded.warnings);
        }
        if !self.record_inputs() {
            recorded.request.body = None;
            recorded.request.messages = None;
        }
        if !self.record_outputs() {
            recorded.content.clear();
            recorded.response.body = None;
            recorded.response.messages.clear();
            recorded.provider_metadata = None;
        }
        recorded
    }

    pub(crate) fn on_error(&self, event: &ErrorEvent<'_>) {
        if !self.options.enabled {
            return;
        }
        let error = (!(self.record_inputs() && self.record_outputs()))
            .then(|| super::redact::redact_error(event.error, &self.options));
        let recorded = ErrorEvent {
            call_id: event.call_id,
            error: error.as_ref().unwrap_or(event.error),
            phase: event.phase,
        };
        for integration in &self.options.integrations {
            integration.on_error(&recorded);
        }
    }

    /// Wraps `call` with every integration's `execute_language_model_call`;
    /// the first integration becomes the outermost wrapper.
    pub(crate) fn execute_language_model_call<'a>(
        &'a self,
        ctx: &'a ModelCallContext,
        call: BoxFuture<'a, Result<ModelCallOutcome, Error>>,
    ) -> BoxFuture<'a, Result<ModelCallOutcome, Error>> {
        if !self.options.enabled {
            return call;
        }
        self.options
            .integrations
            .iter()
            .rev()
            .fold(call, |inner, integration| {
                integration.execute_language_model_call(ctx, inner)
            })
    }

    /// Wraps `call` with every integration's `execute_tool`.
    pub(crate) fn execute_tool<'a>(
        &'a self,
        ctx: &'a ToolExecutionContext,
        call: BoxFuture<'a, Result<ToolOutcome, ToolError>>,
    ) -> BoxFuture<'a, Result<ToolOutcome, ToolError>> {
        if !self.options.enabled {
            return call;
        }
        self.options
            .integrations
            .iter()
            .rev()
            .fold(call, |inner, integration| {
                integration.execute_tool(ctx, inner)
            })
    }
}

impl TelemetryDispatcher {
    dispatch!(on_embed_start, EmbedStartEvent);
    dispatch!(on_embed_end, EmbedEndEvent);
    dispatch!(on_rerank_start, RerankStartEvent);
    dispatch!(on_rerank_end, RerankEndEvent);
}

impl Default for TelemetryDispatcher {
    fn default() -> Self {
        Self::new(TelemetryOptions::default())
    }
}

impl dyn Telemetry {
    /// Convenience for tests: returns `true` when `self` is the same object.
    #[must_use]
    pub fn ptr_eq(this: &Arc<Self>, other: &Arc<Self>) -> bool {
        Arc::ptr_eq(this, other)
    }
}
