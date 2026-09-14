# AGENTS.md

Guidance for AI coding agents and human contributors working in this repository.
It is an operational digest of `docs/03-engineering/`; when the two disagree,
`docs/` wins and this file must be corrected.

## 1. Project and current state

- Ferrin is an AI SDK for Rust applications: text generation, streaming, tools
  with approval, agents, structured output, an MCP client, and the embedding,
  image, speech, transcription, reranking and video modalities.
- All 15 crates under `crates/`, the `xtask` commands and the seven examples
  are implemented (first complete build: 2026-09-14). Version 0.1.0 of every
  crate was published to crates.io on 2026-09-14 (tag `v0.1.0`); each
  changelog has a `[0.1.0]` section and collects further work under
  `Unreleased`. Work in dependency order (`cargo xtask publish-order`): `ferrin-spec` →
  `ferrin-schema` / `ferrin-message` / `ferrin-provider-util` →
  `ferrin-tool` → provider crates and `ferrin-mcp` → `ferrin-core` →
  `ferrin-otel` / `ferrin-testing` → `ferrin`. Each architecture chapter ends
  with dated implementation records (`实现记录`) describing what the code does
  where it refines the design; read them together with the chapter.
- The design documents are the single source of truth: `docs/README.md` is
  the index and states the writing conventions; `docs/01-architecture/` holds
  module contracts, `docs/02-api/` the public API, `docs/03-engineering/` the
  engineering rules, `docs/04-decisions/` the ADRs, `docs/05-appendix/` the
  core-behaviour checklist and pending-verification items. Read the relevant chapter
  before implementing a module. If code and docs disagree, fix the docs first (through an ADR when a
  recorded decision changes), then the code.

## 2. Language

- Design documents, ADRs and the project `README.md` are written in Chinese
  (the root README is the user-facing project page, not a design document;
  crate READMEs are English). Code,
  identifiers, comments, rustdoc, commit messages, CHANGELOG entries, CI and
  configuration files are written in English.
- Every statement in the design documents carries one label: 【事实】 (a fact
  traceable to a source, a specification or a recorded run, with its origin),
  【决策】 (a decision with its technical rationale), or 【待验证】 (a pending
  item with a `PV-xxx` id registered in
  `docs/05-appendix/02-pending-verification.md`). Never describe planned
  capabilities as implemented or verified.
- Record only verified official stable versions, and append a verification
  row to section 5 of `docs/03-engineering/01-toolchain-and-dependencies.md`
  whenever versions are checked or changed.

## 3. Toolchain and commands

- Rust 1.98.1 (`rust-toolchain.toml`), edition 2024, `rust-version = "1.98"`.
  Do not change the toolchain unless the task asks for it, and update the
  toolchain document in the same change.
- Always go through `just`:
  - `just fmt` / `just fmt-check`: rustfmt with
    `--config imports_granularity=Item` (stable rustfmt prints a warning about
    the nightly option; ignore it).
  - `just clippy`: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
  - `just test [-p <crate>]`: nextest with `--all-features`, `--no-fail-fast`
    and `RUST_MIN_STACK=8388608`. Live tests are `#[ignore]`; run them with
    `just test -- --run-ignored only -E 'test(live_)'` plus the provider
    environment variables (`OPENAI_API_KEY`, optional `OPENAI_BASE_URL`,
    `OPENAI_MODEL`, `OPENAI_PROVIDER_OPTIONS`, `OPENAI_COMPATIBLE_*`).
  - `just doctest`: `cargo test --doc` (nextest does not run doc examples).
  - `just bench [filter]`: `cargo bench --workspace --all-features` (criterion;
    HTML report in `target/criterion/report/index.html`). Only a name filter
    can be passed workspace-wide; criterion options such as
    `--save-baseline` need one target:
    `cargo bench -p <crate> --bench <name> -- <options>`.
  - `just doc`, `just api-check` (`cargo xtask api-snapshot --check`),
    `just module-size`, `just deny`, `just shear`, `just features`
    (`cargo hack --each-feature`), `just typos`, `just docs-lint`,
    `just package` (packages every publishable crate in publish order);
    `just check-all` runs all of the above.
  - `just verify`: runs the prototypes in `verification/`.
    `just changelog <crate>`: renders the crate's `Unreleased` section with
    git-cliff.
  - `cargo xtask publish-order`, `cargo xtask check-module-size`,
    `cargo xtask check-versions` (crates.io index; exit code 1 when outdated),
    `cargo xtask record-fixture --provider <p> --case <area>/<name>` (needs
    the provider key in the environment; writes secret-checked fixture files
    from `<name>.scenario.json`), `cargo xtask api-snapshot [--check]`
    (regenerate `docs/api/*.json` after public API changes; CI checks them),
    `python3 scripts/docs_lint.py`.
- Before committing, `just fmt-check`, `just clippy`, `just test`,
  `just doctest`, `just doc` and `just api-check` must pass. Run `just deny` and `just features` when dependencies
  or features change; run `scripts/docs_lint.py` and `typos` when documents
  change.
- `just shear` (`cargo shear --deny-warnings`) must report nothing; the CI
  `shear` job fails otherwise. Remove a dependency from the member manifest
  and from `[workspace.dependencies]` when the last user goes away.

## 4. Code rules

These rules are enforced by the workspace lints (`[workspace.lints]` in
`Cargo.toml`, `clippy.toml`) and CI. Do not silence them with `#[allow]`;
where an exception is unavoidable, put the reason in a comment on the same
line.

- Async trait methods are written as
  `fn name(&self, ..) -> impl Future<Output = T> + Send;`; implementations may
  use `async fn`. `async-trait` is banned by `cargo deny`. When object safety
  is required, provide a hand-written `Dyn*` trait (ADR 0002).
- Banned: `unwrap`/`expect` outside tests, `todo!`, `unimplemented!`, `dbg!`,
  `println!`/`eprintln!` (allowed at crate root in `xtask` and `examples/*`),
  and `unsafe` (`forbid`).
- Banned calls: `tokio::spawn` (use `JoinSet` so tasks are cancelled with their
  owner), `std::env::var` (use `ferrin_provider_util::settings`), and
  `reqwest::Client::{get,post,request,execute}` (all HTTP goes through
  `ferrin_provider_util::http`).
- Errors are `thiserror` enums; library crates never use `anyhow` or `eyre`.
  The core `Error` stays at or below 128 bytes (box large payloads; keep the
  `const_assert!`). Error messages start lowercase, have no trailing period,
  and never contain secrets.
- Public enums are `#[non_exhaustive]`. Constructor naming (`new`, `try_new`,
  `parse`, `builder()`) follows section 3 of the coding standards. Do not
  emulate inheritance with `Deref`.
- Logging uses `tracing` with `#[tracing::instrument(skip_all, fields(..))]`
  at the function definition. Never log secrets, full request bodies or full
  response bodies.
- Inline `format!` arguments, prefer method references to closures, avoid bare
  boolean or `Option` parameters, keep `match` exhaustive.
- Every public item has rustdoc: a one-sentence summary, `# Errors` for
  `Result`-returning functions, `# Panics` where applicable, `# Examples` for
  entry points. `just doc` runs with `-D warnings`.
- Modules target 500 lines and are split above 800
  (`cargo xtask check-module-size` warns). A single change stays under 800
  lines (500 for complex logic); split larger work into reviewable stages.

## 5. Structure and tests

- `src/lib.rs` holds module declarations, `pub use` re-exports and crate docs
  only. Modules are private by default; the public API is re-exported
  explicitly from `lib.rs`.
- Test files never sit next to source files: no `*_tests.rs` under `src/`
  and no `#[cfg(test)] mod tests` inside implementation files. Tests live in
  `tests/suite/*.rs`, aggregated into one binary by `tests/all.rs` (which
  allows `clippy::unwrap_used`/`expect_used` at the crate root). Only logic
  that needs crate-internal visibility is tested from `src/tests/`.
  Benchmarks live in `benches/<name>.rs` (`[[bench]] harness = false`,
  criterion) and allow `clippy::unwrap_used`/`expect_used` at the crate root
  like `tests/all.rs`; they drive `MockLanguageModel` or `FixtureServer`,
  never the network or `sleep`, and their results are not committed
  (`bench.yml` is manual and not a gate).
  Fixtures live in `tests/fixtures/<area>/<case>.*`. `trybuild` cases live in
  `crates/ferrin/tests/ui/`.
- Compare whole objects with `pretty_assertions::assert_eq!`, not field by
  field. Do not test constants. Test helpers belong in `ferrin-testing` or
  `tests/common`, never in implementation files.
- Provider tests replay recorded fixtures (`cargo xtask record-fixture`):
  non-streaming responses through `wiremock`, streaming responses through the
  hyper-based SSE server in `ferrin-testing`. Request bodies are `insta`
  snapshots; CI runs with `INSTA_UPDATE=no`.
- Test case lists named in the design documents are the minimum coverage for
  the corresponding module (for example the `extract_reasoning` list in
  `docs/01-architecture/10-middleware-and-registry.md`).
- nextest runs with `retries = 0`. Fix flaky tests; never add retries or
  `sleep`-based waits.

## 6. Dependencies and features

- External dependencies are declared only in `[workspace.dependencies]` of the
  root `Cargo.toml`; members use `{ workspace = true }` and may only add
  `features`. No `*` versions, no git dependencies, no `[patch]`.
- Before adding a dependency, verify the latest stable version on crates.io,
  record it in the toolchain document, and explain purpose, alternatives and
  maintenance status in the PR. `just deny` (license allow list; `openssl`,
  `native-tls` and `async-trait` banned) must pass.
- Features are strictly additive and never change the shape of existing types.
  The internal crates `ferrin-provider-util` and `ferrin-core` are declared
  with `default-features = false` in the workspace table; the `ferrin` facade
  re-enables their defaults.
- Dependencies deliberately not used, with reasons, are listed in section 2 of
  the toolchain document (`async-trait`, `eventsource-stream`, `infer`,
  `anyhow`, `once_cell`, `lazy_static`, `data-url`, `arc-swap`, `dynosaur`).

## 7. Commits and pull requests

- Conventional Commits with the crate short name as scope: `feat(core): ...`,
  `fix(openai): ...`, `docs: ...`, `refactor(spec): ...`. Breaking changes add
  a `BREAKING CHANGE:` footer.
- Update the `Unreleased` section of every touched crate's `CHANGELOG.md`
  (Keep a Changelog categories). Specification changes in `ferrin-spec` are
  prefixed with `SPEC:` and linked to an ADR.
- Fill in `.github/pull_request_template.md`: motivation, summary of changes,
  testing, breaking or not, related ADR or issue.
- Commit `Cargo.lock`, `insta` snapshots and the `.stderr` snapshots under
  `verification/`; do not commit other local artefacts.
- The project is licensed under Apache-2.0 only (`license` in the workspace
  manifest; `LICENSE` and `NOTICE` are copied into every published crate).
  Code derived from another project is attributed in the root `NOTICE`, in
  the crate documentation of the affected crate and in the module
  documentation of the affected file; keep the three in sync (ADR 0017).

## 8. Security

- Secrets use `secrecy` types and are redacted in logs and errors. Fixtures
  and snapshots must not contain real keys, accounts or internal host names.
- Downloads and MCP endpoints follow the secure URL policy (HTTPS only,
  private networks rejected, DNS pinning, size limits) implemented in
  `ferrin_provider_util::secure_url`; do not bypass it.
- Approval signatures use HMAC-SHA256 with constant-time comparison
  (`Mac::verify_slice` or `subtle`).

## 9. Pending-verification workflow

- When a question cannot be settled from the sources or documents, do not
  guess: write `【待验证】（PV-xxx）` at the point in the source document,
  register the item in the appendix table, and where possible add a
  reproducible prototype under `verification/` (a separate workspace run with
  `just verify`).
- When closing an item, update the source document (relabel as 【事实】 or
  【决策】 with the prototype or origin) and the appendix conclusion and status
  together. `scripts/docs_lint.py` checks ids and registration.
- Open item at the time of writing: PV-031 (provider fixtures are
  hand-written; re-record them with `cargo xtask record-fixture` and real
  credentials, then compare the snapshots).
