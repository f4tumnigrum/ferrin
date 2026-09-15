# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.1] - 2026-09-15

### Changed

- Coordinate workspace version 0.1.1 and re-export the breaking schema
  transformation APIs that now return `Result` (ADR 0019).

### Added

- `end_to_end` benchmark (feature `openai`): `stream_text` through the
  Responses adapter against a fixture server replaying a synthetic text
  stream, single stream and 1/16/64 concurrent streams.

## [0.1.0] - 2026-09-14

### Added

- Root re-exports of the `ferrin-core` API (entry points, result types,
  modules) and of the lower layers as `spec`, `message`, `schema`, `tool`,
  `provider_util`, `serde`, `serde_json` and `schemars`.
- Feature-gated re-exports: `openai`, `anthropic`, `google`,
  `openai_compatible` (also under `providers`), `mcp`, `otel`, `realtime`
  and the `#[ferrin::tool]` attribute macro (`macros`).
- `prelude` with entry points, messages, tools, common specification types,
  the serde derives, `json!` and `StreamExt`.
- `trybuild` compile-pass and compile-fail cases for `#[ferrin::tool]` in
  `tests/ui/`.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
