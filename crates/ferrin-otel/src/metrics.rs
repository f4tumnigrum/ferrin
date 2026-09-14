//! GenAI client metrics.

use std::time::Duration;

use ferrin_core::telemetry::ModelIdentity;
use ferrin_spec::Usage;
use opentelemetry::KeyValue;
use opentelemetry::metrics::Histogram;
use opentelemetry::metrics::Meter;

use crate::semconv;

/// The histograms recorded by [`crate::OtelTelemetry`].
pub(crate) struct Metrics {
    token_usage: Histogram<u64>,
    operation_duration: Histogram<f64>,
    time_to_first_chunk: Histogram<f64>,
    tool_duration: Histogram<f64>,
}

/// A finished GenAI client operation.
pub(crate) struct Operation<'a> {
    pub(crate) name: &'static str,
    pub(crate) model: &'a ModelIdentity,
    pub(crate) response_model: Option<&'a str>,
    pub(crate) duration: Duration,
    pub(crate) error_type: Option<&'a str>,
}

impl Metrics {
    pub(crate) fn new(meter: &Meter) -> Self {
        Self {
            token_usage: meter
                .u64_histogram(semconv::METRIC_TOKEN_USAGE)
                .with_unit("{token}")
                .with_description("Number of input and output tokens used.")
                .with_boundaries(semconv::TOKEN_USAGE_BOUNDARIES.to_vec())
                .build(),
            operation_duration: meter
                .f64_histogram(semconv::METRIC_OPERATION_DURATION)
                .with_unit("s")
                .with_description("GenAI operation duration.")
                .with_boundaries(semconv::DURATION_BOUNDARIES.to_vec())
                .build(),
            time_to_first_chunk: meter
                .f64_histogram(semconv::METRIC_TIME_TO_FIRST_CHUNK)
                .with_unit("s")
                .with_description("Time to receive the first chunk of a streamed response.")
                .with_boundaries(semconv::DURATION_BOUNDARIES.to_vec())
                .build(),
            tool_duration: meter
                .f64_histogram(semconv::METRIC_EXECUTE_TOOL_DURATION)
                .with_unit("s")
                .with_description("The duration of a single tool execution.")
                .with_boundaries(semconv::DURATION_BOUNDARIES.to_vec())
                .build(),
        }
    }

    fn operation_attributes(operation: &Operation<'_>) -> Vec<KeyValue> {
        let mut attributes = vec![
            KeyValue::new(semconv::GEN_AI_OPERATION_NAME, operation.name),
            KeyValue::new(
                semconv::GEN_AI_PROVIDER_NAME,
                operation.model.provider.to_string(),
            ),
            KeyValue::new(
                semconv::GEN_AI_REQUEST_MODEL,
                operation.model.model_id.to_string(),
            ),
        ];
        if let Some(model) = operation.response_model {
            attributes.push(KeyValue::new(
                semconv::GEN_AI_RESPONSE_MODEL,
                model.to_owned(),
            ));
        }
        attributes
    }

    /// Records the duration of an operation (with `error.type` on failure).
    pub(crate) fn record_operation(&self, operation: &Operation<'_>) {
        let mut attributes = Self::operation_attributes(operation);
        if let Some(error_type) = operation.error_type {
            attributes.push(KeyValue::new(semconv::ERROR_TYPE, error_type.to_owned()));
        }
        self.operation_duration
            .record(operation.duration.as_secs_f64(), &attributes);
    }

    /// Records the input and output token counts of an operation.
    pub(crate) fn record_usage(&self, operation: &Operation<'_>, usage: &Usage) {
        self.record_tokens(operation, "input", usage.input.total);
        self.record_tokens(operation, "output", usage.output.total);
    }

    /// Records a single token count.
    pub(crate) fn record_tokens(
        &self,
        operation: &Operation<'_>,
        token_type: &'static str,
        tokens: Option<u64>,
    ) {
        let Some(tokens) = tokens else {
            return;
        };
        let mut attributes = Self::operation_attributes(operation);
        attributes.push(KeyValue::new(semconv::GEN_AI_TOKEN_TYPE, token_type));
        self.token_usage.record(tokens, &attributes);
    }

    /// Records the time to the first streamed chunk.
    pub(crate) fn record_time_to_first_chunk(
        &self,
        operation: &Operation<'_>,
        time_to_first_chunk: Duration,
    ) {
        let attributes = Self::operation_attributes(operation);
        self.time_to_first_chunk
            .record(time_to_first_chunk.as_secs_f64(), &attributes);
    }

    /// Records a tool execution.
    pub(crate) fn record_tool(
        &self,
        tool_name: &str,
        duration: Duration,
        error_type: Option<&'static str>,
    ) {
        let mut attributes = vec![
            KeyValue::new(semconv::GEN_AI_TOOL_NAME, tool_name.to_owned()),
            KeyValue::new(semconv::GEN_AI_TOOL_TYPE, semconv::TOOL_TYPE_FUNCTION),
        ];
        if let Some(error_type) = error_type {
            attributes.push(KeyValue::new(semconv::ERROR_TYPE, error_type));
        }
        self.tool_duration
            .record(duration.as_secs_f64(), &attributes);
    }
}
