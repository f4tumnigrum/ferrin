# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
