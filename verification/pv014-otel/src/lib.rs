//! PV-014: opentelemetry 0.32 + opentelemetry_sdk 0.32 + tracing-opentelemetry
//! 0.33 must compile together (the layer's generic bound ties the versions).

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_semantic_conventions::attribute;
use tracing_subscriber::layer::SubscriberExt;

pub const GEN_AI_ATTRIBUTES: &[&str] = &[
    attribute::GEN_AI_OPERATION_NAME,
    attribute::GEN_AI_PROVIDER_NAME,
    attribute::GEN_AI_REQUEST_MODEL,
    attribute::GEN_AI_RESPONSE_MODEL,
    attribute::GEN_AI_USAGE_INPUT_TOKENS,
    attribute::GEN_AI_USAGE_OUTPUT_TOKENS,
    attribute::GEN_AI_RESPONSE_FINISH_REASONS,
    attribute::GEN_AI_TOOL_NAME,
    attribute::GEN_AI_TOOL_CALL_ID,
];

pub fn build_subscriber() -> impl tracing::Subscriber + Send + Sync {
    let provider = SdkTracerProvider::builder().build();
    let tracer = provider.tracer("ferrin-pv014");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry().with(otel_layer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_compiles_and_records() {
        let subscriber = build_subscriber();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("gen_ai.generate", gen_ai.request.model = "m");
            let _guard = span.enter();
            tracing::info!("hello");
        });
        assert_eq!(GEN_AI_ATTRIBUTES[0], "gen_ai.operation.name");
        println!("gen_ai attributes: {GEN_AI_ATTRIBUTES:?}");
    }
}
