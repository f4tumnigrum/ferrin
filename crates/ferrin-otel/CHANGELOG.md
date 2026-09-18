# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.2.0] - 2026-09-18

### Changed

- Implement the core's awaited telemetry callback contract (ADR 0026).
- Update telemetry integration regression fixtures for the core's optional runtime
  context event field (ADR 0021).
- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Apply call-level `record_outputs` to tool result attributes, including the
  execution-wrapper path.
- Emit successful and failed embedding/reranking `CLIENT` spans alongside metrics.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

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
