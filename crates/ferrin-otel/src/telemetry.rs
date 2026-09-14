//! [`OtelTelemetry`]: the `Telemetry` integration.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Instant;

use ferrin_core::Error;
use ferrin_core::telemetry::AbortEvent;
use ferrin_core::telemetry::EmbedEndEvent;
use ferrin_core::telemetry::EmbedStartEvent;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::ErrorEvent;
use ferrin_core::telemetry::ErrorPhase;
use ferrin_core::telemetry::ModelCallContext;
use ferrin_core::telemetry::ModelCallEndEvent;
use ferrin_core::telemetry::ModelCallOutcome;
use ferrin_core::telemetry::ModelIdentity;
use ferrin_core::telemetry::RerankEndEvent;
use ferrin_core::telemetry::RerankStartEvent;
use ferrin_core::telemetry::Telemetry;
use ferrin_core::telemetry::ToolExecutionContext;
use ferrin_core::telemetry::ToolOutcome;
use ferrin_spec::BoxFuture;
use ferrin_spec::FinishReason;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Usage;
use ferrin_tool::ToolError;
use opentelemetry::Array;
use opentelemetry::Context;
use opentelemetry::InstrumentationScope;
use opentelemetry::KeyValue;
use opentelemetry::StringValue;
use opentelemetry::Value;
use opentelemetry::global;
use opentelemetry::global::BoxedTracer;
use opentelemetry::global::ObjectSafeTracer;
use opentelemetry::metrics::Meter;
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::FutureExt;
use opentelemetry::trace::SpanBuilder;
use opentelemetry::trace::SpanKind;
use opentelemetry::trace::Status;
use opentelemetry::trace::TraceContextExt;
use opentelemetry::trace::Tracer;
use opentelemetry::trace::TracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::metrics::Metrics;
use crate::metrics::Operation;
use crate::semconv;

/// A streamed model call whose span stays open until the end event.
struct PendingCall {
    context: Context,
    model: ModelIdentity,
    started: Instant,
}

/// An embedding or rerank call between its start and end events.
struct ModalityCall {
    operation: &'static str,
    model: ModelIdentity,
    started: Instant,
}

/// OpenTelemetry integration for [`ferrin_core::TelemetryOptions`].
///
/// Model calls become `CLIENT` spans named `chat {model}` and tool executions
/// `INTERNAL` spans named `execute_tool {tool}`, parented to the OpenTelemetry
/// context of the current `tracing` span (through `tracing-opentelemetry`)
/// or, without one, to the current OpenTelemetry context. Streamed model
/// calls keep their span open until the model call end event arrives so the
/// span covers the whole stream. Token usage, operation durations, time to
/// first chunk and tool durations are recorded as GenAI client histograms.
///
/// Span status descriptions and `error.type` carry the low-cardinality error
/// kind, never the error message.
pub struct OtelTelemetry {
    tracer: BoxedTracer,
    metrics: Option<Metrics>,
    record_tool_content: bool,
    pending: Mutex<HashMap<(String, u32), PendingCall>>,
    modality_calls: Mutex<HashMap<String, ModalityCall>>,
}

/// Builder of [`OtelTelemetry`].
pub struct OtelTelemetryBuilder {
    tracer: Option<BoxedTracer>,
    meter: Option<Meter>,
    metrics: bool,
    record_tool_content: bool,
}

fn scope() -> InstrumentationScope {
    InstrumentationScope::builder(semconv::SCOPE_NAME)
        .with_version(env!("CARGO_PKG_VERSION"))
        .build()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn token_value(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

fn tool_error_type(error: &ToolError) -> &'static str {
    match error {
        ToolError::Message { .. } => "message",
        ToolError::Json { .. } => "json",
        ToolError::Timeout(_) => "timeout",
        ToolError::Cancelled => "cancelled",
        #[allow(unreachable_patterns, reason = "the tool error enum is non-exhaustive")]
        _ => "_OTHER",
    }
}

/// The parent of a new span: the current `tracing` span's OpenTelemetry
/// context when the `tracing-opentelemetry` layer is installed, else the
/// current OpenTelemetry context.
fn parent_context() -> Context {
    let from_tracing = tracing::Span::current().context();
    if from_tracing.has_active_span() {
        from_tracing
    } else {
        Context::current()
    }
}

/// Sets the response attributes of a model call span.
fn record_response(
    context: &Context,
    response: &ResponseMetadata,
    finish_reason: &FinishReason,
    usage: &Usage,
) {
    let span = context.span();
    if let Some(id) = &response.id {
        span.set_attribute(KeyValue::new(semconv::GEN_AI_RESPONSE_ID, id.clone()));
    }
    if let Some(model) = &response.model_id {
        span.set_attribute(KeyValue::new(
            semconv::GEN_AI_RESPONSE_MODEL,
            model.to_string(),
        ));
    }
    span.set_attribute(KeyValue::new(
        semconv::GEN_AI_RESPONSE_FINISH_REASONS,
        Value::Array(Array::String(vec![StringValue::from(
            finish_reason.unified.to_string(),
        )])),
    ));
    if let Some(tokens) = usage.input.total {
        span.set_attribute(KeyValue::new(
            semconv::GEN_AI_USAGE_INPUT_TOKENS,
            token_value(tokens),
        ));
    }
    if let Some(tokens) = usage.output.total {
        span.set_attribute(KeyValue::new(
            semconv::GEN_AI_USAGE_OUTPUT_TOKENS,
            token_value(tokens),
        ));
    }
}

/// Ends a span with an error status and `error.type`.
fn end_with_error(context: &Context, error_type: &str) {
    let span = context.span();
    span.set_attribute(KeyValue::new(semconv::ERROR_TYPE, error_type.to_owned()));
    span.set_status(Status::error(error_type.to_owned()));
    span.end();
}

impl OtelTelemetryBuilder {
    /// Uses `provider` for spans instead of the global tracer provider.
    #[must_use]
    pub fn tracer_provider<P>(mut self, provider: &P) -> Self
    where
        P: TracerProvider,
        P::Tracer: ObjectSafeTracer + Send + Sync + 'static,
    {
        self.tracer = Some(BoxedTracer::new(Box::new(
            provider.tracer_with_scope(scope()),
        )));
        self
    }

    /// Uses `provider` for metrics instead of the global meter provider.
    #[must_use]
    pub fn meter_provider<P: MeterProvider + ?Sized>(mut self, provider: &P) -> Self {
        self.meter = Some(provider.meter_with_scope(scope()));
        self
    }

    /// Disables the GenAI client metrics (spans only).
    #[must_use]
    pub fn without_metrics(mut self) -> Self {
        self.metrics = false;
        self
    }

    /// Records tool inputs (`gen_ai.tool.call.arguments`, when the call also
    /// records inputs) and outputs (`gen_ai.tool.call.result`) on tool spans.
    /// Off by default: tool content may contain sensitive data.
    #[must_use]
    pub fn record_tool_content(mut self) -> Self {
        self.record_tool_content = true;
        self
    }

    /// Builds the integration; unset providers default to the global ones.
    #[must_use]
    pub fn build(self) -> OtelTelemetry {
        let tracer = self
            .tracer
            .unwrap_or_else(|| global::tracer_provider().tracer_with_scope(scope()));
        let metrics = self.metrics.then(|| {
            let meter = self
                .meter
                .unwrap_or_else(|| global::meter_provider().meter_with_scope(scope()));
            Metrics::new(&meter)
        });
        OtelTelemetry {
            tracer,
            metrics,
            record_tool_content: self.record_tool_content,
            pending: Mutex::default(),
            modality_calls: Mutex::default(),
        }
    }
}

impl OtelTelemetry {
    /// An integration using the global tracer and meter providers.
    ///
    /// Install the providers (`opentelemetry::global::set_tracer_provider`,
    /// `set_meter_provider`) before calling this: the tracer and meter are
    /// resolved once, here.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use ferrin_core::TelemetryOptions;
    /// use ferrin_otel::OtelTelemetry;
    ///
    /// let options = TelemetryOptions::enabled().with_integration(Arc::new(OtelTelemetry::new()));
    /// assert_eq!(options.integrations.len(), 1);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// A builder for explicit providers and options.
    #[must_use]
    pub fn builder() -> OtelTelemetryBuilder {
        OtelTelemetryBuilder {
            tracer: None,
            meter: None,
            metrics: true,
            record_tool_content: false,
        }
    }

    fn start_span(&self, name: String, kind: SpanKind, attributes: Vec<KeyValue>) -> Context {
        let parent = parent_context();
        let span = self.tracer.build_with_context(
            SpanBuilder::from_name(name)
                .with_kind(kind)
                .with_attributes(attributes),
            &parent,
        );
        parent.with_span(span)
    }

    fn model_call_attributes(ctx: &ModelCallContext) -> Vec<KeyValue> {
        let mut attributes = vec![
            KeyValue::new(semconv::GEN_AI_OPERATION_NAME, semconv::OPERATION_CHAT),
            KeyValue::new(
                semconv::GEN_AI_PROVIDER_NAME,
                ctx.model.provider.to_string(),
            ),
            KeyValue::new(
                semconv::GEN_AI_REQUEST_MODEL,
                ctx.model.model_id.to_string(),
            ),
            KeyValue::new(semconv::FERRIN_CALL_ID, ctx.call_id.clone()),
            KeyValue::new(semconv::FERRIN_STEP_NUMBER, i64::from(ctx.step_number)),
        ];
        if let Some(function_id) = &ctx.function_id {
            attributes.push(KeyValue::new(
                semconv::FERRIN_FUNCTION_ID,
                function_id.clone(),
            ));
        }
        attributes
    }

    fn tool_attributes(&self, ctx: &ToolExecutionContext) -> Vec<KeyValue> {
        let mut attributes = vec![
            KeyValue::new(
                semconv::GEN_AI_OPERATION_NAME,
                semconv::OPERATION_EXECUTE_TOOL,
            ),
            KeyValue::new(semconv::GEN_AI_TOOL_NAME, ctx.tool_name.as_str().to_owned()),
            KeyValue::new(
                semconv::GEN_AI_TOOL_CALL_ID,
                ctx.tool_call_id.as_str().to_owned(),
            ),
            KeyValue::new(semconv::GEN_AI_TOOL_TYPE, semconv::TOOL_TYPE_FUNCTION),
            KeyValue::new(semconv::FERRIN_CALL_ID, ctx.call_id.clone()),
        ];
        if self.record_tool_content
            && let Some(input) = &ctx.input
        {
            attributes.push(KeyValue::new(
                semconv::GEN_AI_TOOL_CALL_ARGUMENTS,
                input.to_string(),
            ));
        }
        attributes
    }

    /// Removes and returns the pending streamed calls of `call_id`.
    fn take_pending(&self, call_id: &str) -> Vec<PendingCall> {
        lock(&self.pending)
            .extract_if(|(id, _), _| id == call_id)
            .map(|(_, call)| call)
            .collect()
    }

    /// Ends the pending streamed calls of `call_id` with an error.
    fn fail_pending(&self, call_id: &str, error_type: &str) {
        for call in self.take_pending(call_id) {
            end_with_error(&call.context, error_type);
            if let Some(metrics) = &self.metrics {
                metrics.record_operation(&Operation {
                    name: semconv::OPERATION_CHAT,
                    model: &call.model,
                    response_model: None,
                    duration: call.started.elapsed(),
                    error_type: Some(error_type),
                });
            }
        }
    }

    /// Records the metrics of a failed embedding or rerank call.
    fn fail_modality(&self, call_id: &str, error_type: &str) {
        let Some(call) = lock(&self.modality_calls).remove(call_id) else {
            return;
        };
        if let Some(metrics) = &self.metrics {
            metrics.record_operation(&Operation {
                name: call.operation,
                model: &call.model,
                response_model: None,
                duration: call.started.elapsed(),
                error_type: Some(error_type),
            });
        }
    }

    fn start_modality(&self, call_id: &str, operation: &'static str, model: &ModelIdentity) {
        lock(&self.modality_calls).insert(
            call_id.to_owned(),
            ModalityCall {
                operation,
                model: model.clone(),
                started: Instant::now(),
            },
        );
    }

    fn end_modality(&self, call_id: &str, duration: std::time::Duration, tokens: Option<u64>) {
        let Some(call) = lock(&self.modality_calls).remove(call_id) else {
            return;
        };
        if let Some(metrics) = &self.metrics {
            let operation = Operation {
                name: call.operation,
                model: &call.model,
                response_model: None,
                duration,
                error_type: None,
            };
            metrics.record_operation(&operation);
            metrics.record_tokens(&operation, "input", tokens);
        }
    }
}

impl Default for OtelTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for OtelTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtelTelemetry")
            .field("metrics", &self.metrics.is_some())
            .field("record_tool_content", &self.record_tool_content)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for OtelTelemetryBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtelTelemetryBuilder")
            .field("tracer", &self.tracer.is_some())
            .field("meter", &self.meter.is_some())
            .field("metrics", &self.metrics)
            .field("record_tool_content", &self.record_tool_content)
            .finish()
    }
}

impl Telemetry for OtelTelemetry {
    fn on_language_model_call_end(&self, event: &ModelCallEndEvent) {
        let pending = lock(&self.pending).remove(&(event.call_id.clone(), event.step_number));
        if let Some(pending) = pending {
            record_response(
                &pending.context,
                &event.response,
                &event.finish_reason,
                &event.usage,
            );
            pending.context.span().end();
        }
        if let Some(metrics) = &self.metrics {
            let operation = Operation {
                name: semconv::OPERATION_CHAT,
                model: &event.model,
                response_model: event
                    .response
                    .model_id
                    .as_ref()
                    .map(ferrin_spec::ModelId::as_str),
                duration: event.performance.response_time,
                error_type: None,
            };
            metrics.record_operation(&operation);
            metrics.record_usage(&operation, &event.usage);
            if let Some(first) = event.performance.time_to_first_output {
                metrics.record_time_to_first_chunk(&operation, first);
            }
        }
    }

    fn on_embed_start(&self, event: &EmbedStartEvent) {
        self.start_modality(&event.call_id, semconv::OPERATION_EMBEDDINGS, &event.model);
    }

    fn on_embed_end(&self, event: &EmbedEndEvent) {
        self.end_modality(&event.call_id, event.duration, event.tokens);
    }

    fn on_rerank_start(&self, event: &RerankStartEvent) {
        self.start_modality(&event.call_id, semconv::OPERATION_RERANK, &event.model);
    }

    fn on_rerank_end(&self, event: &RerankEndEvent) {
        self.end_modality(&event.call_id, event.duration, None);
    }

    fn on_end(&self, event: &EndEvent) {
        for call in self.take_pending(&event.call_id) {
            call.context.span().end();
        }
    }

    fn on_abort(&self, event: &AbortEvent) {
        self.fail_pending(&event.call_id, "cancelled");
    }

    fn on_error(&self, event: &ErrorEvent<'_>) {
        let error_type = event.error.kind().as_str();
        if matches!(event.phase, ErrorPhase::ModelCall | ErrorPhase::Stream) {
            self.fail_pending(event.call_id, error_type);
        }
        self.fail_modality(event.call_id, error_type);
    }

    fn execute_language_model_call<'a>(
        &'a self,
        ctx: &'a ModelCallContext,
        call: BoxFuture<'a, Result<ModelCallOutcome, Error>>,
    ) -> BoxFuture<'a, Result<ModelCallOutcome, Error>> {
        Box::pin(async move {
            let context = self.start_span(
                format!("{} {}", semconv::OPERATION_CHAT, ctx.model.model_id),
                SpanKind::Client,
                Self::model_call_attributes(ctx),
            );
            let started = Instant::now();
            let result = call.with_context(context.clone()).await;
            match &result {
                Ok(ModelCallOutcome::Generate(generated)) => {
                    record_response(
                        &context,
                        &generated.response,
                        &generated.finish_reason,
                        &generated.usage,
                    );
                    context.span().end();
                }
                Ok(ModelCallOutcome::Stream(_)) => {
                    context
                        .span()
                        .set_attribute(KeyValue::new(semconv::FERRIN_STREAMING, true));
                    let previous = lock(&self.pending).insert(
                        (ctx.call_id.clone(), ctx.step_number),
                        PendingCall {
                            context: context.clone(),
                            model: ctx.model.clone(),
                            started,
                        },
                    );
                    if let Some(previous) = previous {
                        end_with_error(&previous.context, "retry");
                    }
                }
                #[allow(unreachable_patterns, reason = "the outcome enum is non-exhaustive")]
                Ok(_) => context.span().end(),
                Err(error) => {
                    let error_type = error.kind().as_str();
                    end_with_error(&context, error_type);
                    if let Some(metrics) = &self.metrics {
                        metrics.record_operation(&Operation {
                            name: semconv::OPERATION_CHAT,
                            model: &ctx.model,
                            response_model: None,
                            duration: started.elapsed(),
                            error_type: Some(error_type),
                        });
                    }
                }
            }
            result
        })
    }

    fn execute_tool<'a>(
        &'a self,
        ctx: &'a ToolExecutionContext,
        call: BoxFuture<'a, Result<ToolOutcome, ToolError>>,
    ) -> BoxFuture<'a, Result<ToolOutcome, ToolError>> {
        Box::pin(async move {
            let context = self.start_span(
                format!("{} {}", semconv::OPERATION_EXECUTE_TOOL, ctx.tool_name),
                SpanKind::Internal,
                self.tool_attributes(ctx),
            );
            let started = Instant::now();
            let result = call.with_context(context.clone()).await;
            let error_type = match &result {
                Ok(outcome) => {
                    if self.record_tool_content {
                        context.span().set_attribute(KeyValue::new(
                            semconv::GEN_AI_TOOL_CALL_RESULT,
                            outcome.output.to_string(),
                        ));
                    }
                    context.span().end();
                    None
                }
                Err(error) => {
                    let error_type = tool_error_type(error);
                    end_with_error(&context, error_type);
                    Some(error_type)
                }
            };
            if let Some(metrics) = &self.metrics {
                metrics.record_tool(ctx.tool_name.as_str(), started.elapsed(), error_type);
            }
            result
        })
    }
}
