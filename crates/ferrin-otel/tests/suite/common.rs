//! Shared harness: in-memory exporters and event constructors.

use std::time::Duration;

use ferrin_core::generate_text::StepPerformance;
use ferrin_core::telemetry::ModelCallContext;
use ferrin_core::telemetry::ModelCallEndEvent;
use ferrin_core::telemetry::ModelIdentity;
use ferrin_core::telemetry::ToolExecutionContext;
use ferrin_otel::OtelTelemetry;
use ferrin_otel::OtelTelemetryBuilder;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Usage;
use opentelemetry::Array;
use opentelemetry::KeyValue;
use opentelemetry::Value;
use opentelemetry_sdk::metrics::InMemoryMetricExporter;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::metrics::data::AggregatedMetrics;
use opentelemetry_sdk::metrics::data::MetricData;
use opentelemetry_sdk::trace::InMemorySpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::trace::SpanData;

/// In-memory span and metric exporters wired to SDK providers.
pub(crate) struct Harness {
    spans: InMemorySpanExporter,
    metrics: InMemoryMetricExporter,
    pub(crate) tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
}

/// One histogram data point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistogramPoint {
    pub(crate) attributes: Vec<KeyValue>,
    pub(crate) count: u64,
    pub(crate) sum: f64,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let spans = InMemorySpanExporter::default();
        let tracer_provider = SdkTracerProvider::builder()
            .with_simple_exporter(spans.clone())
            .build();
        let metrics = InMemoryMetricExporter::default();
        let meter_provider = SdkMeterProvider::builder()
            .with_periodic_exporter(metrics.clone())
            .build();
        Self {
            spans,
            metrics,
            tracer_provider,
            meter_provider,
        }
    }

    pub(crate) fn builder(&self) -> OtelTelemetryBuilder {
        OtelTelemetry::builder()
            .tracer_provider(&self.tracer_provider)
            .meter_provider(&self.meter_provider)
    }

    pub(crate) fn telemetry(&self) -> OtelTelemetry {
        self.builder().build()
    }

    pub(crate) fn finished_spans(&self) -> Vec<SpanData> {
        self.tracer_provider.force_flush().unwrap();
        self.spans.get_finished_spans().unwrap()
    }

    /// Data points of the histogram `name` in the latest export.
    pub(crate) fn histogram_points(&self, name: &str) -> Vec<HistogramPoint> {
        self.meter_provider.force_flush().unwrap();
        let exported = self.metrics.get_finished_metrics().unwrap();
        let Some(latest) = exported.last() else {
            return Vec::new();
        };
        let mut points = Vec::new();
        for scope in latest.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != name {
                    continue;
                }
                match metric.data() {
                    AggregatedMetrics::U64(MetricData::Histogram(histogram)) => {
                        points.extend(histogram.data_points().map(|point| HistogramPoint {
                            attributes: point.attributes().cloned().collect(),
                            count: point.count(),
                            sum: u32::try_from(point.sum()).map(f64::from).unwrap(),
                        }));
                    }
                    AggregatedMetrics::F64(MetricData::Histogram(histogram)) => {
                        points.extend(histogram.data_points().map(|point| HistogramPoint {
                            attributes: point.attributes().cloned().collect(),
                            count: point.count(),
                            sum: point.sum(),
                        }));
                    }
                    _ => {}
                }
            }
        }
        points
    }
}

pub(crate) fn attr<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a Value> {
    attributes
        .iter()
        .find(|pair| pair.key.as_str() == key)
        .map(|pair| &pair.value)
}

pub(crate) fn attr_str(attributes: &[KeyValue], key: &str) -> Option<String> {
    match attr(attributes, key)? {
        Value::String(value) => Some(value.as_str().to_owned()),
        other => panic!("attribute {key} is not a string: {other:?}"),
    }
}

pub(crate) fn attr_strings(attributes: &[KeyValue], key: &str) -> Option<Vec<String>> {
    match attr(attributes, key)? {
        Value::Array(Array::String(values)) => Some(
            values
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
        ),
        other => panic!("attribute {key} is not a string array: {other:?}"),
    }
}

pub(crate) fn model() -> ModelIdentity {
    ModelIdentity::new("openai", "gpt-5")
}

pub(crate) fn model_call_context(step_number: u32) -> ModelCallContext {
    ModelCallContext {
        call_id: "call-1".to_owned(),
        step_number,
        model: model(),
        function_id: Some("docs.explain".to_owned()),
    }
}

pub(crate) fn usage(input: u64, output: u64) -> Usage {
    let mut usage = Usage::default();
    usage.input.total = Some(input);
    usage.output.total = Some(output);
    usage
}

pub(crate) fn response_metadata() -> ResponseMetadata {
    ResponseMetadata {
        id: Some("resp-1".to_owned()),
        model_id: Some("gpt-5-2026".into()),
        ..ResponseMetadata::default()
    }
}

pub(crate) fn generate_result() -> GenerateResult {
    GenerateResult {
        content: Vec::new(),
        finish_reason: FinishReason::stop(),
        usage: usage(12, 7),
        provider_metadata: None,
        request: RequestMetadata::default(),
        response: response_metadata(),
        warnings: Vec::new(),
    }
}

pub(crate) fn model_call_end(
    step_number: u32,
    time_to_first_output: Option<Duration>,
) -> ModelCallEndEvent {
    ModelCallEndEvent {
        runtime_context: None,
        call_id: "call-1".to_owned(),
        step_number,
        model: model(),
        content: None,
        finish_reason: FinishReason::stop(),
        usage: usage(12, 7),
        response: response_metadata(),
        performance: StepPerformance {
            response_time: Duration::from_millis(1500),
            time_to_first_output,
            ..StepPerformance::default()
        },
        warnings: Vec::new(),
    }
}

pub(crate) fn tool_context(input: Option<JsonValue>) -> ToolExecutionContext {
    ToolExecutionContext {
        record_outputs: true,
        call_id: "call-1".to_owned(),
        tool_call_id: "tc-1".into(),
        tool_name: "get_weather".into(),
        input,
    }
}
