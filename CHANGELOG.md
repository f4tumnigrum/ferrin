# Changelog

Workspace-level changelog. Each crate keeps its own `CHANGELOG.md`; this file
lists releases and cross-crate changes. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Workspace skeleton: 15 crates, `xtask`, one example, CI workflows, lint and
  tool configuration, and the `verification/` prototype workspace.
- Shared stream driver in `ferrin-provider-util` (`stream_driver`) used by
  the streaming language models of `ferrin-openai`, `ferrin-anthropic`,
  `ferrin-openai-compatible` and `ferrin-google`.
- `ferrin-mcp`: MCP client with Streamable HTTP, legacy SSE and stdio
  transports, dual protocol-generation negotiation, tool bridging, MCP Apps
  helpers and OAuth (ADR 0015).
- `ferrin-testing`: `Fixture::hold_open` for long-lived event streams.
- `ferrin-otel`: `OtelTelemetry` with GenAI semantic-convention spans and
  client metrics.
- `ferrin` facade: root re-exports, module aliases, feature-gated providers,
  `prelude`, and `#[ferrin::tool]` from `ferrin-macros` with `trybuild`
  cases.
- `xtask`: `check-versions` (crates.io sparse index), `record-fixture`
  (JSON scenarios, secret-checked fixture files) and `api-snapshot`
  (rustdoc JSON public API summaries in `docs/api/`).
- Examples: `example-generate-text`, `example-structured-output`,
  `example-tool-approval`, `example-agent`, `example-mcp`,
  `example-stream-sse-server` and `example-otel`.
- Live tests (`#[ignore]`, `test(live_)`) for the facade, `ferrin-openai`
  and `ferrin-openai-compatible`.
