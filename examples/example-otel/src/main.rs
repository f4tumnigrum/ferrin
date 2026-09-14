//! Exports GenAI semantic-convention spans for a `generate_text` call.
//!
//! The spans go to a stdout exporter so the example has no collector
//! dependency; replace `StdoutExporter` with an OTLP exporter in a real
//! service. A `tracing` span wraps the call to show how Ferrin spans attach to
//! the application's own trace through `tracing-opentelemetry`.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-otel
//! ```

#![allow(clippy::print_stdout)]

use std::sync::Arc;

use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::otel::OtelTelemetry;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::trace::SpanData;
use opentelemetry_sdk::trace::SpanExporter;
use tracing::Instrument;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Prints finished spans with their attributes.
#[derive(Debug)]
struct StdoutExporter;

impl SpanExporter for StdoutExporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        for span in batch {
            let duration = span
                .end_time
                .duration_since(span.start_time)
                .unwrap_or_default();
            println!(
                "span {:?} `{}` ({:?}, {:.1?}, parent {})",
                span.span_kind, span.name, span.status, duration, span.parent_span_id
            );
            for attribute in &span.attributes {
                println!("    {} = {}", attribute.key, attribute.value);
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(StdoutExporter)
        .build();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("example-otel")))
        .init();

    // Spans and metrics use the providers given here; `OtelTelemetry::new()`
    // would read the global providers instead.
    let telemetry = OtelTelemetry::builder()
        .tracer_provider(&provider)
        .without_metrics()
        .build();
    let mut options = TelemetryOptions::enabled().with_integration(Arc::new(telemetry));
    options.function_id = Some("example.otel".to_owned());

    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());
    let model = openai.responses(&model_id);
    let result = async {
        generate_text(model)
            .prompt("Name three uses of OpenTelemetry in one sentence each.")
            .telemetry(options)
            .await
    }
    .instrument(tracing::info_span!("handle_request", request_id = 42))
    .await?;

    println!();
    println!("{}", result.text());

    provider.force_flush()?;
    provider.shutdown()?;
    Ok(())
}
