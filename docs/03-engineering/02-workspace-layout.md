# Workspace layout

**English** | [Chinese](../zh-CN/03-engineering/02-workspace-layout.md)

## 1. Directory structure

```
ferrin/
  Cargo.toml                 # workspace members, package, dependencies, lints, profiles
  Cargo.lock                 # committed
  rust-toolchain.toml
  rustfmt.toml
  clippy.toml
  deny.toml
  typos.toml
  cliff.toml                 # git-cliff configuration
  justfile
  .cargo/config.toml         # cargo xtask alias
  .config/nextest.toml
  .github/workflows/         # ci, typos, semver, coverage, live-tests, versions, release, bench
  .github/dependabot.yml
  .github/pull_request_template.md
  scripts/docs_lint.py       # documentation links and pending-verification checks
  verification/              # independent, unpublished prototype workspace
  crates/
    ferrin-spec/
    ferrin-schema/
    ferrin-message/
    ferrin-provider-util/
    ferrin-tool/
    ferrin-core/
    ferrin-mcp/
    ferrin-otel/
    ferrin-policy/
    ferrin-testing/
    ferrin-macros/
    ferrin/
    providers/
      ferrin-openai/
      ferrin-anthropic/
      ferrin-openai-compatible/
      ferrin-google/
      ferrin-azure/
      ferrin-voyage/
  xtask/
  examples/                  # one unpublished binary crate per example
  docs/                      # primary English documentation; independent Chinese edition in zh-CN/
    00-overview/ 01-architecture/ 02-api/ 03-engineering/ 04-decisions/ 05-appendix/
    README.md                # design index and conventions
    api/                     # generated public API JSON, shared by editions and checked by CI
    providers/               # provider capabilities, options, and behavior
  assets/                    # README banner images; excluded from published packages
  CHANGELOG.md               # workspace changelog; each crate also has one
  CONTRIBUTING.md SECURITY.md
  AGENTS.md                  # operational guidance, subordinate to primary docs
  CLAUDE.md                  # imports AGENTS.md for Claude Code
  LICENSE NOTICE             # Apache-2.0 text and attribution, copied into every published crate
```

[Fact] The 2026-09-13 skeleton included 15 crates (manifest, lib.rs, README, changelog), `xtask`, generate-text example, configuration, and workflows. Check/clippy/doc/deny/hack/nextest passed on 1.98.1.

[Decision] Group crates under `crates/` and `crates/providers/` rather than crowding the repository root as the workspace grows.

[Decision] [ADR 0017](../04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md): Apache-2.0 only; copy root `LICENSE`/`NOTICE` into each published crate because `cargo package` includes only crate-local files and Apache section 4 requires distribution. Keep copies synchronized.

## 2. Root Cargo.toml

```toml
[workspace]
resolver = "3"
members = ["crates/ferrin", "crates/ferrin-*", "crates/providers/*", "xtask", "examples/*"]
exclude = ["verification"]

[workspace.package]
version = "0.1.2"
edition = "2024"
rust-version = "1.98"
license = "Apache-2.0"
repository = "https://github.com/f4tumnigrum/ferrin"
authors = ["Ferrin contributors"]

[workspace.dependencies]
# internal
ferrin-spec = { path = "crates/ferrin-spec", version = "0.1.2" }
ferrin-schema = { path = "crates/ferrin-schema", version = "0.1.2" }
# ... every workspace crate
# external — versions from docs/03-engineering/01-toolchain-and-dependencies.md
tokio = { version = "1.53", default-features = false }
serde = { version = "1.0", features = ["derive"] }
serde_json = { version = "1.0", features = ["preserve_order", "raw_value"] }
# ...

[workspace.lints]
# see docs/03-engineering/03-coding-standards.md

[profile.dev]
debug = "limited"

[profile.release]
lto = "thin"
codegen-units = 4
debug = "line-tables-only"

[profile.ci-test]
inherits = "test"
opt-level = 0
debug = "limited"
```

[Decision] Use dev debug `limited`; release thin LTO, four codegen units, and line-tables-only debug; `ci-test` inherits `test` with smaller output. This balances build speed/performance while retaining line-number backtraces.

[Decision] Resolver 3, the edition-2024 default, enables MSRV-aware resolution.

[Fact] Member globs must match directories containing `Cargo.toml`. `crates/*` incorrectly includes the providers directory; use `crates/ferrin` and `crates/ferrin-*`.

[Fact] Members cannot disable defaults enabled in workspace dependency entries. Declare `ferrin-provider-util`/core with default-features false in the workspace; the facade re-enables them.

[Decision] Declare external versions only in workspace.dependencies; members inherit with workspace true and may add `features`, never independent versions.

## 3. Member manifest template

```toml
[package]
name = "ferrin-core"
description = "Ferrin core: text generation loop, streaming pipeline, agents, middleware."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
readme = "README.md"
keywords = ["ai", "llm", "sdk"]
categories = ["api-bindings", "asynchronous"]

[lib]
name = "ferrin_core"
path = "src/lib.rs"

[lints]
workspace = true

[features]
default = ["video"]
video = []
realtime = ["dep:tokio-tungstenite"]

[dependencies]
ferrin-spec = { workspace = true }
tokio = { workspace = true, features = ["rt", "sync", "time", "macros"] }

[dev-dependencies]
ferrin-testing = { workspace = true }
insta = { workspace = true }

[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

## 4. Source organization

- lib.rs contains only module declarations, public re-exports, and crate docs.
- Modules default private; export APIs explicitly at the root.
- Keep tests outside implementation directories: no sibling `*_tests.rs` or inline test modules.
- Aggregate `tests/suite/*.rs` through `tests/all.rs` into one binary to reduce linking. Allow unwrap/expect at that test crate root because Clippy's test setting does not cover integration-test helpers.
- Internal-only tests live in src/tests, included through a cfg(test) module, with `pub(crate)` subjects; prefer public-API testing.

[Decision] Since 2026-09-13, separate tests from source instead of sibling test files so review and module-size statistics reflect implementation only.
- Fixtures live in `tests/fixtures/<area>/<case>.*`.
- Criterion benches live in `benches/<name>.rs` with harness false, covering hot SSE/JSON/schema/pruning/fingerprint/generation/stream/provider/facade paths; see [Testing](04-testing.md), section 11.

## 5. Examples

Each examples subdirectory is an unpublished binary named `example-<topic>`: generate-text, stream-sse-server, tool-approval, mcp, structured-output, agent, and otel. Examples are documentation; CI compiles all.

- [Fact] On 2026-09-14 all seven were implemented with facade feature selection, Tokio, and `anyhow`, reading provider settings through env_var. OpenAiSettings loads key/base URL lazily; model defaults `gpt-5`. Tool `examples` also read JSON `OPENAI_PROVIDER_OPTIONS`, such as store false. All seven passed against a third-party compatible endpoint, with store false for tools. Generate-text prints text/finish/usage; structured output derives Recipe through facade paths; approval prompts in terminal and appends approved/denied responses; agent defines two macro tools, six-step limit, and step hooks; MCP connects to `server-everything` over stdio (command/args overridable) and calls `get-sum`; SSE uses hyper/`hyper-util`/`http-body-util`, `JoinSet`-owned connections, split events as data JSON and final DONE; OTel uses a custom stdout `SpanExporter`, SDK provider/tracing bridge, without_metrics, and a parent tracing span. CI compiles but does not run `examples` requiring credentials.

## 6. `xtask`

```
cargo xtask record-fixture --provider <name> --case <case>
cargo xtask check-versions
cargo xtask publish-order          # prints crates in dependency order
cargo xtask api-snapshot           # dumps public API via rustdoc JSON for review
cargo xtask check-module-size      # warns on non-test files above 800 lines
```

The cargo xtask alias is defined in `.cargo/config.toml` as run --quiet --package xtask --.

- [Fact] All five subcommands were implemented on 2026-09-14 in publish_order/module_size/check_versions/record_fixture and api_snapshot/{mod,summary,render}, with shared metadata/publish/runtime helpers in `workspace.rs`. Publish order uses Kahn sorting of normal/build dependencies, alphabetic ties; module size counts source files excluding *`_tests.rs`.
- [Fact] `check-versions` resolves direct external crates.io dependencies (highest locked version when duplicated), fetches sparse-index entries concurrently through default_transport/`JoinSet`, and compares highest unyanked stable versions. Markdown output lists outdated crates or all-current status; outdated versions or failed lookups exit nonzero for `versions.yml` issues. First run: all 52 current.
- [Decision] record-fixture reads area/name.scenario.json, replacing the earlier Rust/TOML plan because `serde_json` is already available and a scenario describes one HTTP request. Fields: `method` (`POST` default), `path` with query, `base_url`, `api_key_env`, `headers`, `body`, `stream`, `model`; reject unknown fields. Defaults: OpenAI api.`openai`.com/v1 with Bearer `OPENAI_API_KEY`; Anthropic api.`anthropic`.com/v1 with `x-api-key` and version 2023-06-01; Google generativelanguage.googleapis.com/v1beta with `x-goog-api-key`; compatible scenarios must supply URL/key variable. Read keys through settings; send via RecordingTransport. Retain only `content-type`, request IDs, retry `headers`, OpenAI processing/version, and `provider` rate-limit `headers`; `body` limit 64 MiB. Write request JSON, response JSON or encoded SSE chunks, and metadata `status`/`headers`/`recorded_at`/`provider`/`case`/`model`. Scan every output for secrets and the actual key before writing. Case paths permit only ASCII alphanumerics, hyphen, underscore, slash.
- [Decision] api-snapshot runs all-feature rustdoc JSON for each `publish-order` crate using `RUSTC_BOOTSTRAP=1` and unstable-options on pinned stable 1.98.1, reading target/`doc` and writing docs/api. Check mode compares without writes and fails on differences. Require supported `format_version` 60 or report a parser update requirement. Traverse public modules, recording paths/kinds/signatures (qualifiers, generics, where clauses, alias/constant types, trait bounds/dyn compatibility), `non_exhaustive`/`must_use`/`repr`/`deprecated`, public fields/variants/trait items/inherent methods, non-synthetic non-blanket trait impls, and re-export sources. Exclude docs/private items. Snapshots aid review, not semver judgment.
- [Fact] After first CI on 2026-09-14, differing auto-trait order between macOS and Linux caused snapshot mismatches. `render_poly_traits` now retains the primary trait and sorts the rest; check failures print up to 40 differing lines for diagnosis.

## 7. `justfile`

```just
set working-directory := "."

fmt:
    cargo fmt --all -- --config imports_granularity=Item

fmt-check:
    cargo fmt --all -- --config imports_granularity=Item --check

clippy *args:
    cargo clippy --workspace --all-targets {{args}} -- -D warnings

fix *args:
    cargo clippy --workspace --all-targets --fix --allow-dirty {{args}}

test *args:
    RUST_MIN_STACK=8388608 NEXTEST_PROFILE=local cargo nextest run --workspace --no-fail-fast {{args}}

doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features

deny:
    cargo deny check

shear:
    cargo shear --deny-warnings

features:
    cargo hack check --workspace --each-feature --no-dev-deps

typos:
    typos

check-all: fmt-check clippy test doc deny shear

verify *args:                       # runs the verification/ prototypes
    cargo test --manifest-path verification/Cargo.toml --workspace --no-fail-fast -- --nocapture --test-threads=1 {{args}}

changelog crate:                    # per-crate changelog fragment from Conventional Commits
    git cliff --config cliff.toml --include-path "crates/{{crate}}/**" --unreleased
```

[Decision] `just test` sets `RUST_MIN_STACK=8388608` and `NEXTEST_PROFILE=local`, with nextest no-fail-fast. The documented fix entry invokes Clippy fixes with dirty-tree allowance. Eight-MiB stacks prevent deeply nested future/property-test overflow and match CI; no-fail-fast exposes all failures in one run.
