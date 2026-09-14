//! Ferrin OpenTelemetry bridge.
//!
//! [`OtelTelemetry`] implements [`ferrin_core::Telemetry`]: it creates
//! OpenTelemetry spans named after the GenAI semantic conventions for model
//! calls and tool executions (parented to the current `tracing` span through
//! `tracing-opentelemetry`) and records the GenAI client metrics
//! (`gen_ai.client.token.usage`, `gen_ai.client.operation.duration`,
//! `gen_ai.client.operation.time_to_first_chunk`,
//! `gen_ai.execute_tool.duration`).
//!
//! Attribute and metric names live in [`semconv`]; Ferrin-specific attributes
//! use the `ferrin.*` prefix.
//!
//! Design: `docs/01-architecture/13-observability.md`.
//!
//! # Examples
//!
//! ```
//! use std::sync::Arc;
//!
//! use ferrin_core::TelemetryOptions;
//! use ferrin_otel::OtelTelemetry;
//!
//! // Install the global OpenTelemetry providers first, then build the
//! // integration and attach it to a call's telemetry options.
//! let telemetry = TelemetryOptions::enabled().with_integration(Arc::new(OtelTelemetry::new()));
//! assert!(telemetry.enabled);
//! ```

mod metrics;
pub mod semconv;
mod telemetry;

pub use telemetry::OtelTelemetry;
pub use telemetry::OtelTelemetryBuilder;
