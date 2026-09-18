# Changelog

Workspace-level changelog. Each crate keeps its own `CHANGELOG.md`; this file
lists releases and cross-crate changes. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.2.0] - 2026-09-18

This is a breaking release across all 18 crates. Follow the
[0.1.2 to 0.2.0 migration guide](docs/02-api/02-api-reference.md#migrating-from-012-to-020).
The [reference review](docs/05-appendix/03-reference-parity.md) records remaining
differences; this release does not claim complete AI SDK parity. Live provider
verification remains limited by PV-031.

### Added

- Independent full/text/partial/element stream views, agent call-options schemas,
  and embedding/reranking operation hooks with runtime context and telemetry.
- Streaming HTTP and multipart uploads, reference provider-tool schema parsing,
  and OAuth authorization-server credential binding and state/issuer validation.
- Azure OpenAI and Voyage reranking providers, Google Interactions and Live
  transcription/translation, and optional facade provider features (ADRs 0023, 0025).
- Independent Agent runtime context, persistent message/instruction/tool context,
  and tool metadata propagation (ADR 0021).

- Fixture recording supports explicit JSON-pointer response redaction, including SSE payloads, with redaction provenance in fixture metadata.
- Recording scenarios can read private endpoints through `base_url_env`; status output omits the endpoint URL.

### Changed

- Coordinate workspace crates and versioned internal dependencies at 0.2.0.
- **Breaking:** embeddings use `f64`; `Instructions` is an enum; step preparation
  exposes the model instance; telemetry callbacks return `BoxFuture`; streaming
  completion actively drives the pipeline; usage and warnings aggregate steps.
- **Breaking:** OpenAI adapters preserve supplied schemas without implicit strict
  transformation; tools select context by name; policy input/shadow behavior and
  MCP protocol defaults follow the documented reference contracts. Request-body
  materialization now returns `Result`.
- **Breaking:** new fields on public core/tool event structs and OpenAI configuration
  require Rust struct literals to be updated; new serialized fields default when
  reading older JSON. Step message/instruction/context overrides now persist.

### Fixed

- Align provider options, resource paths, usage, tool schemas, Google Interactions
  and Realtime, Azure authorization precedence and Voyage reranking behavior.
- Correct callback ordering, metadata propagation, partial output, cancellation,
  retry boundaries, JSON parsing and standalone feature dependency declarations.
- Complete OpenAI advanced provider-tool mapping/replay and OpenAI/Anthropic
  caller/deferred factory bindings (ADR 0022).

## [0.1.2] - 2026-09-16

### Added

- `ferrin-policy`: policy-based tool approval with OPA-style decision
  documents, `HttpPolicyClient` for the OPA REST Data API, `RegoPolicyClient`
  behind the `rego` feature, shadow mode, default statuses and a capability
  middleware; facade features `policy` and `policy-rego` (ADR 0020).
- `ferrin-core`: embedding and image model middleware, `wrap_provider`, and
  registry support for embedding and image middleware.

### Fixed

- Enforce middleware capability restrictions during local tool execution and
  synchronize tool-choice validation in generated and streamed responses.
- Keep policy decision reasons, evaluation errors and URL credentials out of
  automatic diagnostics.
- `semver.yml` excludes crates that do not exist in the baseline tag, because
  `cargo semver-checks` aborts for packages missing from the baseline.

## [0.1.1] - 2026-09-15

### Security

- Update the independent verification workspace to `rustls` 0.23.45 as well,
  removing RUSTSEC-2026-0285 from the prototype dependency lockfile; include
  prototype advisories in local, CI and weekly audits to prevent missed updates.

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

- Coordinate all workspace crates and versioned internal dependencies at 0.1.1.
- **Breaking:** schema transformation APIs now return `Result`; callers must
  handle unsupported strict dictionaries explicitly (ADR 0019). This release
  retains the explicitly selected version 0.1.1 and is not backward API
  compatible with 0.1.0.

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
