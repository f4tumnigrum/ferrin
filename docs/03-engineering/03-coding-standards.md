# Coding standards

**English** | [Chinese](../zh-CN/03-engineering/03-coding-standards.md)

These standards apply to every workspace crate. Rustfmt, Clippy, and CI enforce most rules; review covers the remainder.

## 1. Formatting

[Decision] Set edition 2024 and imports_granularity Item; the latter needs nightly rustfmt, but CI and `just fmt-check` pass it explicitly. Item-level imports make diffs smaller and reduce conflicts.

```toml
# rustfmt.toml
edition = "2024"
imports_granularity = "Item"   # nightly-only option; stable rustfmt warns and ignores, CI passes it explicitly
```

- One imported item per `use` line; no globs except within `prelude` modules.
- Line width 100, the rustfmt default.

## 2. Lint configuration

[Decision] Workspace Clippy denies `await_holding_invalid_type`/lock, `disallowed_methods`, `expect_used`, `identity_op`, manual/needless families, `redundant_clone`/closure/closure_for_method_calls/static_lifetimes, `trivially_copy_pass_by_ref`, `uninlined_format_args`, unnecessary families, and `unwrap_used`. The original draft allowed test `unwrap`/`expect`, listed Tokio mutex guards as invalid across await, and set large-error-threshold 256; the configuration below records the implemented threshold. Shared rules prevent lock-related deadlocks, library panics, and redundant work.

[Decision] Add the following Rust compiler lints:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"                 # "deny" via RUSTFLAGS in CI for published crates
unreachable_pub = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }
unused_qualifications = "warn"
missing_debug_implementations = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(docsrs)"] }   # `just doc` passes --cfg docsrs

[workspace.lints.clippy]
# base set (all "deny"), listed above
# Ferrin additions
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
todo = "deny"
unimplemented = "deny"
large_futures = "warn"
```

```toml
# clippy.toml
allow-expect-in-tests = true
allow-unwrap-in-tests = true
await-holding-invalid-types = [
    "tokio::sync::MutexGuard",
    "tokio::sync::RwLockReadGuard",
    "tokio::sync::RwLockWriteGuard",
]
large-error-threshold = 128
disallowed-methods = [
    { path = "reqwest::Client::get", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::post", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::request", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::execute", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::get", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "std::env::var", reason = "Use ferrin_provider_util::settings for environment lookups." },
    { path = "tokio::spawn", reason = "Use JoinSet so tasks are cancelled with their owner." },
]
```

Provider utility HTTP/`settings` modules locally allow disallowed_methods with explanations. CLI `xtask`/examples allow stdout/stderr at crate roots with reasons.

Unsafe is forbidden everywhere. Any future necessary `unsafe`, such as FFI, needs an ADR, a crate-level downgrade to `deny`, and individually justified allow/SAFETY annotations.

## 3. Naming and API shape

[Decision] Inline format arguments, collapse nested ifs, prefer method references, avoid boolean/bare `Option` parameters, annotate positional literals with parameter names, `match` exhaustively, document trait roles/obligations, and use `impl Future + Send` instead of async_trait. Clippy handles mechanical rules; review ensures readable call sites and contracts.

Additional conventions:

- Public enums are non_exhaustive; internal matches remain exhaustive, while downstream callers need wildcards.
- Use `new` for simple infallible construction, `try_new` or `parse` for fallible construction, and builder for builders.
- `From`/`Into` are lossless; `TryFrom` is fallible. Do not emulate inheritance with `Deref`.
- Builder generics carry output types, not required-field typestate machines.

## 4. Async

- Trait methods return `impl Future<Output = T> + Send`; implementations may use `async fn`.
- Supply Dyn traits when object safety is needed ([Provider specification](../01-architecture/04-provider-spec.md)).
- No `block_on` in library code.
- Use `JoinSet`; `tokio::spawn` is lint-banned.
- [Decision] Revised 2026-09-14 by [ADR 0016](../04-decisions/2026-09-14-0016-inline-encoding-no-spawn-blocking.md): encode/serialize in async tasks without `spawn_blocking`/`block_in_place`, replacing the earlier blocking-pool rule. Measurable blocking workloads require a new ADR.
- Instrument function definitions with tracing::instrument(`skip_all`, fields(...)), rather than call-site instrumentation. `skip_all` avoids automatic capture of secrets or bodies.

## 5. Error handling

- Libraries use `thiserror`, never `anyhow`/`eyre`.
- No `unwrap`/`expect`; unreachable branches use unreachable with a reason.
- Lowercase messages, no trailing period or sensitive data.
- Use `From` for ? conversions; allocate errors only when a failure actually occurs.

## 6. Module and change size

[Decision] Target 500 implementation lines per module; split beyond about 800. Keep changes under 800 lines, or 500 for complex logic, splitting larger work into reviewable stages. These limits reflect review context capacity and are checked by xtask.

CI warns, without blocking, about non-test files over 800 lines; explain exceptions in review.

## 7. Dependencies

- Explain purpose, alternatives, and maintenance of new dependencies in PRs; `cargo deny` must pass.
- No published `git` dependencies. Local patch.crates-io debugging must not be committed.
- No wildcard versions.
- Features are additive ([Crate boundaries](../01-architecture/02-crates.md), section 5).

## 8. Logging

- Use `tracing`, not `log` macros or println/eprintln.
- Error: unrecoverable internal inconsistency; `warn`: provider warnings/fallback; `info`: lifecycle without content; `debug`: request/response metadata; `trace`: chunks.
- Never log secrets or complete bodies; opt-in input/output content uses its dedicated target.

## 9. Rustdoc

- Every public item needs a one-sentence summary, necessary detail, Errors for `Result`, Panics when relevant, and Examples for entry points.
- Traits document when to implement and implementer obligations.
- Module docs explain responsibility and invariants.

## 10. Test code

- Compare whole objects with pretty_assertions::assert_eq for complete diffs.
- Do not test constants or add negative tests for deleted logic.
- Tests may use `unwrap`/`expect`.
- Test-only helpers belong in `ferrin-testing` or `tests/suite/common.rs`, never implementation files.
- Keep tests in tests/suite, separate from source ([Workspace layout](02-workspace-layout.md), section 4).

## 11. Commits and PRs

- Conventional Commits use crate short scopes: feat(core), fix(openai), docs, refactor(spec).
- PRs include motivation, summary, testing, breaking status, and related ADR/issue.
- Breaking changes need a BREAKING CHANGE footer and the affected crate changelog update.
