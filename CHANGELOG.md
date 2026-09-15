# Changelog

Workspace-level changelog. Each crate keeps its own `CHANGELOG.md`; this file
lists releases and cross-crate changes. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Security

- Update transitive `rustls` to 0.23.45 to fix TLS 1.3 handshake encryption-level
  validation (RUSTSEC-2026-0285).

### Added

- Complete English documentation as the primary edition, with an independent
  Chinese README and documentation tree, language navigation, and bilingual
  link and pending-verification checks (ADR 0018).

- Criterion benchmark suite (`just bench`, manual `bench.yml` workflow) covering
  SSE decoding, partial JSON repair, schema derivation and validation, message
  pruning, tool fingerprints, the generation and streaming pipelines, the
  OpenAI, Anthropic and Google adapters against the fixture server, and
  end-to-end streaming through the facade; `criterion` 0.8.2 is a workspace
  dev-dependency.

### Changed

- Architecture diagrams in the English and Chinese documentation now use
  Mermaid, with the Chinese edition maintained independently.

- `release.yml` publishes all not-yet-published crates in one multi-package
  `cargo publish` invocation, waits out crates.io's new-crate rate limit
  (HTTP 429) and retries, and `ferrin-testing` is a path-only workspace
  dev-dependency so published manifests no longer reference it.

### Fixed

- Preserve cargo-deny failures in the weekly advisory workflow so security issues are created.

- `cargo deny` no longer rejects the path-only `ferrin-testing`
  dev-dependency as a wildcard version (`allow-wildcard-paths = true`); the
  `deny` CI job had failed since that dependency change.

## [0.1.0] - 2026-09-14

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

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0): `LICENSE-MIT`
  removed, `LICENSE-APACHE` renamed to `LICENSE`, and a `NOTICE` file added
  attributing the code derived from the Vercel AI SDK; both files are copied
  into every published crate (ADR 0017).
