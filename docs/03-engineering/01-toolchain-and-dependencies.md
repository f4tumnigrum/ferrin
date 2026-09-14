# Toolchain and dependency versions

**English** | [Chinese](../zh-CN/03-engineering/01-toolchain-and-dependencies.md)

Versions verified on 2026-09-13, using the methods below. The design selected the latest official stable versions on that date; recheck and update this document when implementation begins.

## 1. Rust toolchain

| Item | Version | Verification |
| --- | --- | --- |
| Rust stable | 1.98.1 | `rustup check` reported `1.98.1 (48a229cea 2026-09-01)`; `rust-lang/rust` Releases lists 1.98.1 on September 3 and 1.98.0 on August 20. |
| Installed locally | `1.98.1-aarch64-apple-darwin`; rustc 1.98.1 (48a229cea 2026-09-01), cargo 1.98.1 (797e8a9bc 2026-08-05); default stable still 1.98.0 | Rechecked with `rustup toolchain list` and `rustup run 1.98.1 rustc --version` on 2026-09-13. Repository `rust-toolchain.toml` selects 1.98.1; all quality gates ran with it. |
| rustup | 1.29.1 | `rustup --version` |
| Edition | 2024 | Stable since Rust 1.85; latest edition at verification. |

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.98.1"
components = ["clippy", "rustfmt", "rust-src", "llvm-tools-preview"]
profile = "minimal"
```

See [Versioning and release](06-versioning-and-release.md) for MSRV policy.

## 2. Runtime dependencies

Versions come from crates.io API `max_stable_version` on 2026-09-13. These are workspace dependency targets, declared at `major.minor` precision and locked by `Cargo.lock`.

| Crate | Version | Purpose | Consumers |
| --- | --- | --- | --- |
| `tokio` | 1.53.1 | Async runtime: `rt`, `sync`, `time`, `macros`, `net`, `fs`, `process` | All |
| `tokio-util` | 0.7.19 | `CancellationToken` and codecs | spec, core, provider-util |
| `tokio-stream` | 0.1.19 | Channel-to-stream adapters | core |
| `futures-core` | 0.3.34 | `Stream` trait; the only futures dependency in public API | spec |
| `futures-util` | 0.3.34 | Stream combinators | core, provider-util |
| `pin-project-lite` | 0.2.17 | Custom future/stream projection | core |
| `serde` | 1.0.229 | Serialization with `derive` | All |
| `serde_json` | 1.0.151 | JSON with `preserve_order` and `raw_value` | All |
| `schemars` | 1.2.2 | JSON Schema derivation | schema |
| `jsonschema` | 0.56.0 | Optional dynamic JSON Schema validation | schema |
| `thiserror` | 2.0.20 | Error derives | All |
| `bytes` | 1.12.1 | Binary data | All |
| `http` | 1.5.0 | `HeaderMap`, `StatusCode`, `Method` | spec, provider-util |
| `url` | 2.5.8 | URL parsing | spec, provider-util |
| `reqwest` | 0.13.5 | Default HTTP transport: `rustls`/`http2`/`stream`, defaults disabled. [Fact] Since 0.13, `rustls` replaces `rustls-tls` (PV-015). [Decision] No `multipart`/`json`/`charset` since 2026-09-13; see HTTP chapter, section 11. | provider-util |
| `rustls` | 0.23.44 | TLS through reqwest, default aws-lc. [Fact] Removed as a direct workspace dependency on 2026-09-14 because no member uses it directly. | Transitive only |
| `rustls-platform-verifier` | 0.7.0 | System certificate verifier. [Fact] Default in reqwest 0.13; `platform-verifier` feature permits direct configuration. `webpki-roots` is unnecessary. | provider-util feature |
| `tokio-tungstenite` | 0.30.0 | Realtime WebSockets | core `realtime` feature, openai |
| `chrono` | 0.4.45 | Timestamps with `serde`/`clock`; `Retry-After` date parsing | spec, provider-util |
| `rand` | 0.10.2 | Prefixed random IDs | provider-util |
| `base64` | 0.23.1 | Base64 and base64url | spec, provider-util, core |
| `data-url` | 0.3.2 | [Decision] Not adopted (PV-002); `ferrin-message` implements RFC 2397 | — |
| `hmac` | 0.13.0 | Approval signatures | core |
| `sha2` | 0.11.0 | Approval signatures and fingerprints | core, tool, mcp |
| `secrecy` | 0.10.3 | Secret types | provider-util, core, providers |
| `zeroize` | 1.9.0 | [Decision] No direct dependency since 2026-09-13; `secrecy` brings zeroing transitively | — |
| `ipnet` | 2.12.2 | Private subnet checks | provider-util |
| `indexmap` | 2.14.2 | Ordered tool sets | tool |
| `regex` | 1.13.1 | Custom smoothing segmentation and reasoning tags | core |
| `unicode-segmentation` | 1.13.3 | Word boundaries | core |
| `tracing` | 0.1.44 | Logs and spans | All |
| `arc-swap` | 1.9.2 | [Decision] Not adopted (PV-024): default registry uses one-time `OnceLock` (ADR 0008), with immutable middleware/registries and no hot replacement | — |
| `opentelemetry` | 0.32.0 | OTel API | otel |
| `opentelemetry_sdk` | 0.32.1 | Test-only SDK/in-memory exporters through `testing` feature; production depends on API only | otel dev |
| `opentelemetry-semantic-conventions` | 0.32.1 | [Decision] Not adopted (PV-014); GEN_AI constants deprecated, defined locally instead | — |
| `tracing-opentelemetry` | 0.33.0 | Tracing/OTel bridge | otel |
| `syn` / `quote` / `proc-macro2` | 3.0.5 / 1.0.47 / 1.0.107 | Procedural macros; crates.io verified 2026-09-13. [Fact] `syn` 3 coexists with ecosystem `syn` 2. | macros |
| `hyper` / `hyper-util` / `http-body-util` | 1.11.1 / 0.1.20 / 0.1.5 | Streaming fixture server (PV-026) | testing |

[Decision] Excluded dependencies: `async-trait` (RPITIT plus Dyn adapters), `eventsource-stream` (local SSE), `infer` (local signatures), `anyhow` (libraries exclude it; examples/xtask may use it), `once_cell` and `lazy_static` (stable standard `OnceLock`/`LazyLock`).

## 3. Development and testing dependencies

| Crate | Version | Purpose |
| --- | --- | --- |
| `wiremock` | 0.6.5 | HTTP mock server and fixture replay |
| `insta` | 1.48.0 | Request/event snapshots |
| `pretty_assertions` | 1.4.1 | Readable diffs |
| `proptest` | 1.11.0 | Partial JSON/SSE property tests |
| `criterion` | 0.8.2 | Benchmarks with `async_tokio`/`html_reports`; see [Testing](04-testing.md), section 11 |
| `trybuild` | 1.0.121 | Procedural macro compile-fail cases |
| `tracing-subscriber` | 0.3.23 | Test log capture |
| `static_assertions` | 1.1.0 | `Error` size assertions (PV-013) |
| `anyhow` / `clap` / `cargo_metadata` | 1.0.104 / 4.6.6 / 0.23.1 | `xtask`/examples only |

## 4. Command-line tools

| Tool | Version | Purpose |
| --- | --- | --- |
| `cargo-nextest` | 0.9.144 | Test runner, `just test` |
| `cargo-insta` | 1.48.0 | Snapshot review |
| `cargo-deny` | 0.20.2 | License, advisory, duplicate dependency checks |
| `cargo-shear` | 1.13.4 | Unused dependencies; CI uses --deny-warnings |
| `cargo-semver-checks` | 0.50.0 | Pre-release API compatibility |
| `cargo-llvm-cov` | 0.9.1 | Coverage |
| `cargo-hack` | 0.6.45 | Feature/MSRV matrix checks |
| `typos-cli` | 1.50.1 | Rust-based spell checking without Python runtime |
| `release-plz` | 0.3.165 | [Decision] Not adopted (PV-025); see [Versioning and release](06-versioning-and-release.md), section 5 |
| `git-cliff` | 2.14.1 | Changelog generation via `cliff.toml` and just changelog |
| `just` | 1.58.0 | Task entry point |
| `cargo-binstall` | 1.23.0 | Prebuilt tool installation |

## 5. Version verification records

Append a row for every version check:

| Date | Scope | Result | Stage |
| --- | --- | --- | --- |
| 2026-09-13 | Rust and all runtime/dev dependencies | Initial record | Design |
| 2026-09-13 | Toolchain, section 4 tool installation, skeleton lockfile (396 packages), `syn`/`quote`/`proc-macro2`/`hyper` family/`static_assertions`/`anyhow`/`clap`/`cargo_metadata` | All match; four cargo deny checks passed | Preparation |
| 2026-09-14 | OTel API/SDK/tracing bridge, crates.io `max_stable_version` | 0.32.0 / 0.32.1 / 0.33.0, matching document and lockfile | OTel implementation |
| 2026-09-14 | All 52 direct external dependencies, `check-versions` against highest unyanked stable sparse-index versions | All locked versions latest stable | Initial `check-versions` run |
| 2026-09-14 | Shear cleanup removed unused workspace `assert_matches`, `async-stream`, `criterion`, `data-url`, `rustls`, `serde_with`, `subtle`, `tempfile`, `tokio-test`, `uuid`; IDs use IdGenerator and HMAC verify_slice provides constant-time checks. Removed unused core `subtle`/`insta`/`proptest`/`tokio-test`/`assert_matches`/`tracing-subscriber`/`criterion`, provider-util `percent-encoding`/`tracing`/`proptest`/`tokio-test`, testing `futures-core`/`thiserror`/`tracing`, compatible `futures-util`/`regex`; message `serde_json` became dev-only. | No shear warnings | Cleanup |
| 2026-09-14 | Reintroduced `criterion` 0.8.2, latest stable on crates.io (released 2026-02-04, Apache-2.0 OR MIT, MSRV 1.86), as dev dependency for nine crates | Shear, four deny checks, each-feature checks passed; bench workflow uses pinned checkout/toolchain/cache/upload actions | Benchmarks |

check-versions compares manifest dependencies with crates.io and reports outdated entries; weekly CI opens an issue (see [CI and quality gates](05-ci-and-quality-gates.md)).

## 6. Verified API facts

Verified by compiling/running prototypes in verification on 2026-09-13:

- [Fact] (PV-015) reqwest 0.13 changes defaults to `rustls`, aws-lc, system verifier; renames `rustls-tls` to `rustls`, makes `query`/`form` optional, removes deprecated `trust-dns` and others, and renames TLS builders with soft deprecations. Address pinning/no-redirect APIs remain; 0.13.5 adds is_dns and `http1_max_headers`.
- [Fact] (PV-022) `hmac` 0.13, `sha2` 0.11, `subtle` 2.6, `secrecy` 0.10 compile together; `new_from_slice` comes from `hmac::KeyInit`.
- [Fact] (PV-023) `base64` 0.23 retains `Engine` and standard/URL-safe-no-pad prelude constants. `rand` 0.10 uses rng and `RngExt`::sample_iter(Alphanumeric), renaming `Rng` to `RngExt`. `tokio-tungstenite` 0.30 offers `rustls-tls-webpki-roots`/native-roots, default `connect`/`handshake`, `Connector`, and `MaybeTlsStream`.
- [Fact] (PV-014) OTel 0.32.0, SDK 0.32.1, and tracing bridge 0.33.0 are compatible.
- [Fact] (PV-004) `schemars` 1.2.2 draft07 output matches expected shapes; `jsonschema` 0.56 draft-07 validation is covered by implementation tests, without a separate prototype.
- [Fact] Duplicate majors reported by deny: `base64` 0.22/0.23, `rand`/`rand_core` 0.9/0.10, `syn` 2/3, `getrandom` 0.3/0.4. [Decision] Keep latest design versions and multiple-versions warn; let upstream reqwest/OTel upgrades remove duplication rather than downgrading.
- [Fact] Local Cargo configuration uses `sparse+https://rsproxy.cn/index/` for crates.io; resolved versions match official API results.
- [Fact] Pre-push check on 2026-09-14: pinned actions match checkout v7.0.1, rust-toolchain v1, rust-cache v2.9.2, install-action v2.87.12, cargo-deny-action v2.1.1, codecov v7.0.0, typos v1.50.1, create-issue-from-file v6.0.0, action-gh-release v3.0.3; added semver-checks-action v2.9 at `6b69fcf4…`. Cargo update dry-run found transitive patches `cc` 1.4.6, `fancy-regex` 0.19.2, `lru-slab` 0.1.3, `tinyvec` 1.13.3, left for weekly Dependabot; direct versions remain latest.
- [Fact] After removing Codecov upload on 2026-09-14, coverage uses upload-artifact v7.0.1 at `043fb46d…` (verified latest through GitHub API) to retain lcov; no workflows reference Codecov.
