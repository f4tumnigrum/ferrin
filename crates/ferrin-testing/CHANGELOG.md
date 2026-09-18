# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.2.0] - 2026-09-18

### Changed

- Record streaming uploads as transport consumes chunks, preserving cancellation
  and partial-body behavior without eagerly buffering requests (ADR 0026).

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Fixed

- Treat zero-count fixture mounts as absent routes.

- Reject reused stream part IDs after their end events in the contract checker.

## [0.1.0] - 2026-09-14

### Added

- `MockLanguageModel` and `MockLanguageModelBuilder` (`generate`,
  `generate_error`, `generate_repeat`, `generate_with`, `stream`,
  `stream_error`, `stream_repeat`, `stream_with`, `supported_urls`), with
  recorded calls (`calls`, `generate_calls`, `stream_calls`).
- `simulate_stream`, `SimulatedStream` (initial and per-chunk delays,
  `hang_at_end`) and `text_parts`.
- `StreamContractChecker` and `ContractViolation`.
- `Fixture::hold_open` keeps an event-stream response open after the last
  event, for long-lived server-to-client streams.
- `FixtureServer` (single hyper 1.x backend for JSON and SSE fixtures,
  `mount`, `mount_once`, `mount_times`, `mount_file`, per-chunk delays,
  received-request inspection), `Fixture`, `FixtureBody`, fixture file
  encoding helpers.
- `RecordingTransport`, `RecordedRequest`, `RecordedResponse`,
  `HeaderFilter`, `redact_secrets`, `contains_secret`.
- `SequentialIdGenerator`, `api_call_error`.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
