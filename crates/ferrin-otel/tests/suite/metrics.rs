//! GenAI client metrics recorded from lifecycle events.

use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::Telemetry;
use ferrin_core::telemetry::EmbedEndEvent;
use ferrin_core::telemetry::EmbedStartEvent;
use ferrin_core::telemetry::ErrorEvent;
use ferrin_core::telemetry::ErrorPhase;
use ferrin_core::telemetry::RerankEndEvent;
use ferrin_core::telemetry::RerankStartEvent;
use pretty_assertions::assert_eq;

use super::common::Harness;
use super::common::HistogramPoint;
use super::common::attr;
use super::common::attr_str;
use super::common::model;
use super::common::model_call_end;

fn point_with<'a>(points: &'a [HistogramPoint], key: &str, value: &str) -> &'a HistogramPoint {
    points
        .iter()
        .find(|point| attr_str(&point.attributes, key) == Some(value.to_owned()))
        .unwrap_or_else(|| panic!("no data point with {key}={value}: {points:?}"))
}

#[tokio::test]
async fn model_call_end_records_usage_duration_and_time_to_first_chunk() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    telemetry
        .on_language_model_call_end(&model_call_end(0, Some(Duration::from_millis(300))))
        .await;

    let usage = harness.histogram_points("gen_ai.client.token.usage");
    assert_eq!(usage.len(), 2);
    let input = point_with(&usage, "gen_ai.token.type", "input");
    assert_eq!((input.count, input.sum), (1, 12.0));
    let output = point_with(&usage, "gen_ai.token.type", "output");
    assert_eq!((output.count, output.sum), (1, 7.0));
    assert_eq!(
        attr_str(&input.attributes, "gen_ai.operation.name"),
        Some("chat".to_owned())
    );
    assert_eq!(
        attr_str(&input.attributes, "gen_ai.provider.name"),
        Some("openai".to_owned())
    );
    assert_eq!(
        attr_str(&input.attributes, "gen_ai.request.model"),
        Some("gpt-5".to_owned())
    );
    assert_eq!(
        attr_str(&input.attributes, "gen_ai.response.model"),
        Some("gpt-5-2026".to_owned())
    );

    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(durations.len(), 1);
    assert_eq!((durations[0].count, durations[0].sum), (1, 1.5));
    assert_eq!(attr(&durations[0].attributes, "error.type"), None);

    let first_chunk = harness.histogram_points("gen_ai.client.operation.time_to_first_chunk");
    assert_eq!(first_chunk.len(), 1);
    assert_eq!(first_chunk[0].count, 1);
    assert!((first_chunk[0].sum - 0.3).abs() < 1e-9);
}

#[tokio::test]
async fn a_non_streamed_call_records_no_time_to_first_chunk() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    telemetry
        .on_language_model_call_end(&model_call_end(0, None))
        .await;
    assert!(
        harness
            .histogram_points("gen_ai.client.operation.time_to_first_chunk")
            .is_empty()
    );
    assert_eq!(
        harness
            .histogram_points("gen_ai.client.operation.duration")
            .len(),
        1
    );
}

#[tokio::test]
async fn embed_and_rerank_events_record_operation_metrics() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    telemetry
        .on_embed_start(&EmbedStartEvent {
            call_id: "embed-1".to_owned(),
            model: model(),
            value_count: 3,
            values: None,
        })
        .await;
    telemetry
        .on_embed_end(&EmbedEndEvent {
            call_id: "embed-1".to_owned(),
            embedding_count: 3,
            tokens: Some(40),
            duration: Duration::from_millis(250),
        })
        .await;
    telemetry
        .on_rerank_start(&RerankStartEvent {
            call_id: "rerank-1".to_owned(),
            model: model(),
            document_count: 5,
            query: None,
        })
        .await;
    telemetry
        .on_rerank_end(&RerankEndEvent {
            call_id: "rerank-1".to_owned(),
            ranked_count: 5,
            duration: Duration::from_millis(120),
        })
        .await;

    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(durations.len(), 2);
    let embed = point_with(&durations, "gen_ai.operation.name", "embeddings");
    assert_eq!((embed.count, embed.sum), (1, 0.25));
    let rerank = point_with(&durations, "gen_ai.operation.name", "rerank");
    assert_eq!((rerank.count, rerank.sum), (1, 0.12));

    let usage = harness.histogram_points("gen_ai.client.token.usage");
    assert_eq!(usage.len(), 1);
    assert_eq!((usage[0].count, usage[0].sum), (1, 40.0));
    assert_eq!(
        attr_str(&usage[0].attributes, "gen_ai.token.type"),
        Some("input".to_owned())
    );
    assert_eq!(
        attr_str(&usage[0].attributes, "gen_ai.operation.name"),
        Some("embeddings".to_owned())
    );
    let spans = harness.finished_spans();
    assert_eq!(
        spans
            .iter()
            .map(|span| (span.name.as_ref(), span.span_kind.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("embeddings gpt-5", opentelemetry::trace::SpanKind::Client),
            ("rerank gpt-5", opentelemetry::trace::SpanKind::Client)
        ]
    );
    assert_eq!(
        attr(&spans[0].attributes, "gen_ai.usage.input_tokens"),
        Some(&opentelemetry::Value::I64(40))
    );
}

#[tokio::test]
async fn a_failed_modality_call_records_the_duration_with_the_error_type() {
    let harness = Harness::new();
    let telemetry = harness.telemetry();
    telemetry
        .on_embed_start(&EmbedStartEvent {
            call_id: "embed-1".to_owned(),
            model: model(),
            value_count: 1,
            values: None,
        })
        .await;
    let error = Error::NoOutputGenerated;
    telemetry
        .on_error(&ErrorEvent {
            call_id: "embed-1",
            error: &error,
            phase: ErrorPhase::ModelCall,
        })
        .await;
    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(durations.len(), 1);
    assert_eq!(
        attr_str(&durations[0].attributes, "gen_ai.operation.name"),
        Some("embeddings".to_owned())
    );
    assert_eq!(
        attr_str(&durations[0].attributes, "error.type"),
        Some("output".to_owned())
    );
    let spans = harness.finished_spans();
    assert_eq!(
        spans[0].status,
        opentelemetry::trace::Status::error("output")
    );
}

#[tokio::test]
async fn metrics_can_be_disabled() {
    let harness = Harness::new();
    let telemetry = harness.builder().without_metrics().build();
    telemetry
        .on_language_model_call_end(&model_call_end(0, Some(Duration::from_millis(300))))
        .await;
    assert!(
        harness
            .histogram_points("gen_ai.client.token.usage")
            .is_empty()
    );
    assert!(
        harness
            .histogram_points("gen_ai.client.operation.duration")
            .is_empty()
    );
}

#[tokio::test]
async fn parallel_embedding_chunks_record_all_request_and_token_metrics() {
    use ferrin_core::TelemetryOptions;
    use ferrin_core::embed_many;
    use ferrin_spec::EmbeddingModel;
    use ferrin_spec::ModelId;
    use ferrin_spec::ProviderId;
    use ferrin_spec::embedding_model::EmbedOptions;
    use ferrin_spec::embedding_model::EmbedResult;
    use ferrin_spec::embedding_model::EmbeddingUsage;
    use ferrin_spec::error::ProviderError;
    use std::sync::Arc;
    use tokio::sync::Barrier;

    struct ParallelEmbeddings {
        provider: ProviderId,
        model: ModelId,
        barrier: Barrier,
    }
    impl EmbeddingModel for ParallelEmbeddings {
        fn provider(&self) -> &ProviderId {
            &self.provider
        }
        fn model_id(&self) -> &ModelId {
            &self.model
        }
        fn max_embeddings_per_call(&self) -> Option<usize> {
            Some(1)
        }
        fn supports_parallel_calls(&self) -> bool {
            true
        }
        async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
            // Both start events must occur before either end event.
            self.barrier.wait().await;
            Ok(EmbedResult {
                embeddings: options.values.iter().map(|_| vec![1.0]).collect(),
                usage: Some(EmbeddingUsage { tokens: 10 }),
                warnings: Vec::new(),
                provider_metadata: None,
                response: Default::default(),
            })
        }
    }
    let harness = Harness::new();
    let model = Arc::new(ParallelEmbeddings {
        provider: ProviderId::new("mock"),
        model: ModelId::new("embedding"),
        barrier: Barrier::new(2),
    });
    let result = embed_many(model, ["a", "b"])
        .max_parallel_calls(2)
        .telemetry(TelemetryOptions::enabled().with_integration(Arc::new(harness.telemetry())))
        .await
        .unwrap();
    let usage = harness.histogram_points("gen_ai.client.token.usage");
    let durations = harness.histogram_points("gen_ai.client.operation.duration");
    assert_eq!(result.usage.tokens, Some(20));
    assert_eq!(
        usage
            .iter()
            .map(|point| (point.count, point.sum))
            .collect::<Vec<_>>(),
        vec![(2, 20.0)]
    );
    assert_eq!(
        durations
            .iter()
            .map(|point| point.count)
            .collect::<Vec<_>>(),
        vec![2]
    );
}
