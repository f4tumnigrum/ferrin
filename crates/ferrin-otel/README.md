# ferrin-otel

OpenTelemetry integration for Ferrin: `OtelTelemetry` implements
`ferrin_core::Telemetry`, creating GenAI semantic-convention spans for model
calls (`chat {model}`, kind `CLIENT`) and tool executions
(`execute_tool {tool}`, kind `INTERNAL`) and recording the GenAI client
histograms `gen_ai.client.token.usage`, `gen_ai.client.operation.duration`,
`gen_ai.client.operation.time_to_first_chunk` and
`gen_ai.execute_tool.duration`. Attribute and metric names are defined in
`ferrin_otel::semconv`.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/13-observability.md`.

## Example

```rust
use std::sync::Arc;

use ferrin_core::TelemetryOptions;
use ferrin_otel::OtelTelemetry;

// Install your OpenTelemetry tracer and meter providers first
// (`opentelemetry::global::set_tracer_provider` / `set_meter_provider`),
// or pass them explicitly through `OtelTelemetry::builder()`.
let telemetry = TelemetryOptions {
    record_outputs: true,
    function_id: Some("docs.explain".to_owned()),
    ..TelemetryOptions::enabled()
}
.with_integration(Arc::new(OtelTelemetry::new()));
```

Pass `telemetry` to a call through its builder (`.telemetry(..)`). New spans
are parented to the OpenTelemetry context of the current `tracing` span when
the `tracing-opentelemetry` layer is installed, otherwise to the current
OpenTelemetry context. Streamed model calls keep their span open until the
model call end event, so the span covers the whole stream.

Span status descriptions and `error.type` carry the low-cardinality error
kind, never the error message. Tool inputs and outputs are only recorded on
spans after `OtelTelemetry::builder().record_tool_content()`.

## Features

None. `opentelemetry_sdk` is a test-only dependency; applications choose
their own SDK, exporters and processors.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
