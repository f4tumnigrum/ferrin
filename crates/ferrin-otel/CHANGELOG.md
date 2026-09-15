# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.1] - 2026-09-15

### Fixed

- Added an integration regression covering token and request metrics for
  concurrent core embedding chunks with independent correlation IDs.

## [0.1.0] - 2026-09-14

### Added

- `OtelTelemetry` (`ferrin_core::Telemetry` implementation): `CLIENT` spans
  `chat {model}` for model calls with response id, response model, finish
  reasons and token usage; `INTERNAL` spans `execute_tool {tool}` for tool
  executions; streamed calls keep their span open until the model call end
  event; error status and `error.type` from the error kind.
- GenAI client metrics `gen_ai.client.token.usage`,
  `gen_ai.client.operation.duration`,
  `gen_ai.client.operation.time_to_first_chunk` and
  `gen_ai.execute_tool.duration` with the recommended bucket boundaries,
  including embedding and rerank operations.
- `OtelTelemetryBuilder` (`tracer_provider`, `meter_provider`,
  `without_metrics`, `record_tool_content`) and the `semconv` constants.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
