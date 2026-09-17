//! Span creation for model calls and tool executions.

use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::Telemetry;
use ferrin_core::telemetry::AbortEvent;
use ferrin_core::telemetry::ErrorEvent;
use ferrin_core::telemetry::ErrorPhase;
use ferrin_core::telemetry::ModelCallOutcome;
use ferrin_core::telemetry::ToolOutcome;
use ferrin_spec::StreamResult;
use ferrin_tool::ToolError;
use opentelemetry::Value;
use opentelemetry::trace::SpanKind;
use opentelemetry::trace::Status;
use opentelemetry::trace::TracerProvider as _;
use pretty_assertions::assert_eq;
use serde_json::json;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;

use super::common::Harness;
use super::common::attr;
use super::common::attr_str;
use super::common::attr_strings;
use super::common::generate_result;
use super::common::model_call_context;
use super::common::model_call_end;
use super::common::tool_context;

#[tokio::test]
async fn a_generate_call_produces_a_client_span_with_response_attributes() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = model_call_context(2);
    let outcome = telemetry
        .execute_language_model_call(
            &ctx,
            Box::pin(async { Ok(ModelCallOutcome::Generate(Box::new(generate_result()))) }),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, ModelCallOutcome::Generate(_)));

    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    let span = &spans[0];
    assert_eq!(span.name, "chat gpt-5");
    assert_eq!(span.span_kind, SpanKind::Client);
    assert_eq!(span.status, Status::Unset);
    let attributes = &span.attributes;
    assert_eq!(
        attr_str(attributes, "gen_ai.operation.name"),
        Some("chat".to_owned())
    );
    assert_eq!(
        attr_str(attributes, "gen_ai.provider.name"),
        Some("openai".to_owned())
    );
    assert_eq!(
        attr_str(attributes, "gen_ai.request.model"),
        Some("gpt-5".to_owned())
    );
    assert_eq!(
        attr_str(attributes, "gen_ai.response.model"),
        Some("gpt-5-2026".to_owned())
    );
    assert_eq!(
        attr_str(attributes, "gen_ai.response.id"),
        Some("resp-1".to_owned())
    );
    assert_eq!(
        attr_strings(attributes, "gen_ai.response.finish_reasons"),
        Some(vec!["stop".to_owned()])
    );
    assert_eq!(
        attr(attributes, "gen_ai.usage.input_tokens"),
        Some(&Value::I64(12))
    );
    assert_eq!(
        attr(attributes, "gen_ai.usage.output_tokens"),
        Some(&Value::I64(7))
    );
    assert_eq!(
        attr_str(attributes, "ferrin.call_id"),
        Some("call-1".to_owned())
    );
    assert_eq!(attr(attributes, "ferrin.step_number"), Some(&Value::I64(2)));
    assert_eq!(
        attr_str(attributes, "ferrin.function_id"),
        Some("docs.explain".to_owned())
    );
    assert_eq!(attr(attributes, "ferrin.streaming"), None);
    assert_eq!(attr(attributes, "error.type"), None);
}

#[tokio::test]
async fn a_failed_model_call_sets_the_error_status_and_records_the_duration() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = model_call_context(0);
    let result = telemetry
        .execute_language_model_call(&ctx, Box::pin(async { Err(Error::Cancelled) }))
        .await;
    assert!(matches!(result, Err(Error::Cancelled)));

    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].status, Status::error("cancelled"));
    assert_eq!(
        attr_str(&spans[0].attributes, "error.type"),
        Some("cancelled".to_owned())
    );

    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(durations.len(), 1);
    assert_eq!(durations[0].count, 1);
    assert_eq!(
        attr_str(&durations[0].attributes, "error.type"),
        Some("cancelled".to_owned())
    );
    assert_eq!(
        attr_str(&durations[0].attributes, "gen_ai.operation.name"),
        Some("chat".to_owned())
    );
}

#[tokio::test]
async fn a_stream_call_keeps_the_span_open_until_the_end_event() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = model_call_context(0);
    let stream = StreamResult::new(Box::pin(futures_util::stream::empty()));
    telemetry
        .execute_language_model_call(
            &ctx,
            Box::pin(async move { Ok(ModelCallOutcome::Stream(Box::new(stream))) }),
        )
        .await
        .unwrap();
    assert!(harness.finished_spans().is_empty());

    telemetry
        .on_language_model_call_end(&model_call_end(0, Some(Duration::from_millis(300))))
        .await;
    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    let attributes = &spans[0].attributes;
    assert_eq!(
        attr(attributes, "ferrin.streaming"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        attr_str(attributes, "gen_ai.response.id"),
        Some("resp-1".to_owned())
    );
    assert_eq!(
        attr(attributes, "gen_ai.usage.output_tokens"),
        Some(&Value::I64(7))
    );
    assert_eq!(spans[0].status, Status::Unset);
}

#[tokio::test]
async fn an_abort_ends_a_pending_stream_span_with_an_error() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = model_call_context(1);
    let stream = StreamResult::new(Box::pin(futures_util::stream::empty()));
    telemetry
        .execute_language_model_call(
            &ctx,
            Box::pin(async move { Ok(ModelCallOutcome::Stream(Box::new(stream))) }),
        )
        .await
        .unwrap();
    telemetry
        .on_abort(&AbortEvent {
            call_id: "call-1".to_owned(),
            steps_completed: 1,
        })
        .await;

    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].status, Status::error("cancelled"));
    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(durations.len(), 1);
    assert_eq!(
        attr_str(&durations[0].attributes, "error.type"),
        Some("cancelled".to_owned())
    );
}

#[tokio::test]
async fn a_stream_error_ends_the_pending_span_with_the_error_kind() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = model_call_context(0);
    let stream = StreamResult::new(Box::pin(futures_util::stream::empty()));
    telemetry
        .execute_language_model_call(
            &ctx,
            Box::pin(async move { Ok(ModelCallOutcome::Stream(Box::new(stream))) }),
        )
        .await
        .unwrap();
    let error = Error::NoOutputGenerated;
    telemetry
        .on_error(&ErrorEvent {
            call_id: "call-1",
            error: &error,
            phase: ErrorPhase::Stream,
        })
        .await;
    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(
        attr_str(&spans[0].attributes, "error.type"),
        Some("output".to_owned())
    );
}

#[tokio::test]
async fn a_tool_execution_produces_an_internal_span_without_content_by_default() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = tool_context(Some(json!({"city": "Berlin"})));
    let outcome = telemetry
        .execute_tool(
            &ctx,
            Box::pin(async {
                Ok(ToolOutcome {
                    output: json!({"temperature": 21}),
                })
            }),
        )
        .await
        .unwrap();
    assert_eq!(outcome.output, json!({"temperature": 21}));

    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    let span = &spans[0];
    assert_eq!(span.name, "execute_tool get_weather");
    assert_eq!(span.span_kind, SpanKind::Internal);
    assert_eq!(span.status, Status::Unset);
    assert_eq!(
        attr_str(&span.attributes, "gen_ai.operation.name"),
        Some("execute_tool".to_owned())
    );
    assert_eq!(
        attr_str(&span.attributes, "gen_ai.tool.name"),
        Some("get_weather".to_owned())
    );
    assert_eq!(
        attr_str(&span.attributes, "gen_ai.tool.call.id"),
        Some("tc-1".to_owned())
    );
    assert_eq!(
        attr_str(&span.attributes, "gen_ai.tool.type"),
        Some("function".to_owned())
    );
    assert_eq!(attr(&span.attributes, "gen_ai.tool.call.arguments"), None);
    assert_eq!(attr(&span.attributes, "gen_ai.tool.call.result"), None);

    let durations = harness.histogram_points("gen_ai.execute_tool.duration");
    assert_eq!(durations.len(), 1);
    assert_eq!(
        attr_str(&durations[0].attributes, "gen_ai.tool.name"),
        Some("get_weather".to_owned())
    );
    assert_eq!(attr(&durations[0].attributes, "error.type"), None);
}

#[tokio::test]
async fn tool_content_is_recorded_when_enabled() {
    let harness = Harness::new();
    let telemetry = harness.builder().record_tool_content().build();
    let ctx = tool_context(Some(json!({"city": "Berlin"})));
    telemetry
        .execute_tool(
            &ctx,
            Box::pin(async {
                Ok(ToolOutcome {
                    output: json!({"temperature": 21}),
                })
            }),
        )
        .await
        .unwrap();
    let spans = harness.finished_spans();
    assert_eq!(
        attr_str(&spans[0].attributes, "gen_ai.tool.call.arguments"),
        Some(r#"{"city":"Berlin"}"#.to_owned())
    );
    assert_eq!(
        attr_str(&spans[0].attributes, "gen_ai.tool.call.result"),
        Some(r#"{"temperature":21}"#.to_owned())
    );
}

#[tokio::test]
async fn tool_content_respects_call_output_recording() {
    let harness = Harness::new();
    let telemetry = harness.builder().record_tool_content().build();
    let mut ctx = tool_context(None);
    ctx.record_outputs = false;
    telemetry
        .execute_tool(
            &ctx,
            Box::pin(async {
                Ok(ToolOutcome {
                    output: json!({"secret": "redacted"}),
                })
            }),
        )
        .await
        .unwrap();
    let spans = harness.finished_spans();
    assert_eq!(attr(&spans[0].attributes, "gen_ai.tool.call.result"), None);
}

#[tokio::test]
async fn a_failed_tool_execution_records_the_error_type() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    let ctx = tool_context(None);
    let result = telemetry
        .execute_tool(
            &ctx,
            Box::pin(async { Err(ToolError::Timeout(Duration::from_secs(1))) }),
        )
        .await;
    assert!(matches!(result, Err(ToolError::Timeout(_))));
    let spans = harness.finished_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].status, Status::error("timeout"));
    assert_eq!(
        attr_str(&spans[0].attributes, "error.type"),
        Some("timeout".to_owned())
    );
    let durations = harness.histogram_points("gen_ai.execute_tool.duration");
    assert_eq!(
        attr_str(&durations[0].attributes, "error.type"),
        Some("timeout".to_owned())
    );
}

#[tokio::test]
async fn spans_are_children_of_the_current_tracing_span() {
    let harness = Harness::new();
    let tracer = harness.tracer_provider.tracer("test");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = tracing::subscriber::set_default(subscriber);
    let telemetry = harness.telemetry();
    let ctx = model_call_context(0);
    {
        let span = tracing::info_span!("ferrin.model_call");
        telemetry
            .execute_language_model_call(
                &ctx,
                Box::pin(async { Ok(ModelCallOutcome::Generate(Box::new(generate_result()))) }),
            )
            .instrument(span)
            .await
            .unwrap();
    }
    let spans = harness.finished_spans();
    let child = spans.iter().find(|span| span.name == "chat gpt-5").unwrap();
    let parent = spans
        .iter()
        .find(|span| span.name == "ferrin.model_call")
        .unwrap();
    assert_eq!(child.parent_span_id, parent.span_context.span_id());
    assert_eq!(
        child.span_context.trace_id(),
        parent.span_context.trace_id()
    );
}
