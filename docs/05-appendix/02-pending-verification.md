# Pending verification

**English** | [Chinese](../zh-CN/05-appendix/02-pending-verification.md)

This table tracks pending items across the documents. Closing an item updates its status/conclusion and source text together. Reproducible prototypes live in the independent verification workspace, run with `just verify`; benchmark prototypes use cargo run --release. Unless a row states otherwise, measurements are from 2026-09-13 on macOS arm64, Darwin 25.2.0, eight cores, 16 GiB, Rust 1.98.1.

Status: `closed` means source markers became Fact/Decision; `open` means a numbered Pending verification item remains.

| ID | Question | Source | Conclusion | Status |
| --- | --- | --- | --- | --- |
| PV-001 | Dynosaur 0.3.1 Send/static object adapters | Architecture section 5, ADR 0002 | Prototype succeeds across `JoinSet`, but generated unsized structs need explicit construction/`?Sized`. Keep handwritten Dyn traits. | closed |
| PV-002 | Data URL parser versus naive comma splitting | Prompt conversion section 7 | Fourteen cases show naive parsing misses base64 flags/percent decoding. Implement RFC 2397 locally. | closed |
| PV-003 | Default parallel downloads | Prompt section 7, concurrency section 8 | Default 8 via `DownloadOptions::max_parallel`; track in download benchmarks. | closed |
| PV-004 | Draft-07 optional/`enum` shapes and strict schema subset | Tools section 10, output section 7, ADR 0004 | Prototype verifies shapes and strict transform adding no-extra-properties, all-`required`, and nullability. | closed |
| PV-005 | Nonstatic captures in tool macros | Tools section 10 | Recorded E0597/lifetime diagnostics point to user code; reject reference parameters at macro input. | closed |
| PV-006 | `JoinSet`/channel ordering and memory above 100 tools | Generation section 5, concurrency section 8 | At 200/1000 tasks, capacities 1/64/1024 have equal timing/RSS (2.9/6.7 MiB); completion order. Keep 64. | closed |
| PV-007 | Simulated-stream startup and eager variant | Generation section 5, ADR 0005 | Document inherent full-generation wait; no eager variant. | closed |
| PV-008 | Partial-value deep equality versus hashing | Output section 7 | For 96 KiB: equality 135 µs, serialize/hash 180 µs, parsing 936 µs. Keep equality. | closed |
| PV-009 | Clearing agent defaults in `prepare_call` | Agent section 5 | Override prototype superseded on 2026-09-13 by populated defaults with `None` removal; ADR 0013 item 2, Agent section 6. | closed |
| PV-010 | Reasoning extraction test completeness | Middleware section 4 | Fourteen minimum cases: five generation, nine streaming. | closed |
| PV-011 | Embedding byte-limit measurement | Modalities section 12 | UTF-8 bytes, implemented in `embed::split_by_limits` and `embed_many_splits_by_input_bytes`. | closed |
| PV-012 | Realtime event inventory | Modalities section 12 | Original inventory records 22 server and eight client kinds; full enumerated set retained in that chapter. | closed |
| PV-013 | `Error` size ≤128 bytes | Errors section 5, ADR 0006 | 360 inline, 56 with six boxed payloads; static assertion passes. | closed |
| PV-014 | OTel 0.32 with tracing bridge 0.33 | Observability section 6, toolchain section 6 | Prototype compiles/records spans; deprecated GenAI constants replaced locally. | closed |
| PV-015 | Reqwest pinning/redirect APIs and client cost | HTTP section 11, toolchain section 6, ADR 0009 | APIs retained, ~58 µs per client, no DNS after pinning; 0.13 changes documented. | closed |
| PV-016 | Non-ASCII `http` 1.5 header values | HTTP section 11 | Constructors accept UTF-8 bytes, to_str rejects them; expose byte reads. | closed |
| PV-017 | MCP HTTP headers/resumption/version constants | MCP sections 2.2/5, ADR 0010 | `2026-07-28` removes sessions/GET/resumption/server requests, adds method/name/parameter headers and MRTR; implement both generations. | closed |
| PV-018 | Windows stdio pipes/signals | MCP section 5 | kill-on-drop/single writer/creation flags passed all three stdio tests in Windows run 34797869442 on 2026-09-14, using Python fixtures. | closed |
| PV-019 | Elicitation schema fidelity | MCP section 5 | message/requestedSchema and action/optional content map directly. | closed |
| PV-020 | Sleep reset overhead | Concurrency section 8 | 73 ns per `chunk` versus 17 ns `Instant::now`; retain design. | closed |
| PV-021 | OpenAI compatibility flags | Provider guide section 8 | Retain explicit message type and web-search-source include flags for Azure/Mantle differences. | closed |
| PV-022 | HMAC/SHA/`subtle`/SecretBox compatibility | Toolchain section 6, security section 8 | Crypto prototype compiles; import KeyInit for `new_from_slice`; `verify_slice` works. | closed |
| PV-023 | Base64/`rand`/WebSocket APIs | Toolchain section 6 | `Engine`/`prelude` unchanged; `rand` rng/`RngExt`; tungstenite rustls features verified. Duplicate majors accepted as warnings. | closed |
| PV-024 | Need for `arc-swap` | Toolchain section 2 | No: default registry uses `OnceLock` without hot replacement. | closed |
| PV-025 | `release-plz` versus `git-cliff` | Toolchain section 4, releases section 5 | Use `git-cliff`; `release-plz` groups only changed packages, conflicting with synchronized versions. | closed |
| PV-026 | Wiremock delayed SSE support | Testing section 9 | No streaming API; hyper prototype delivers three 50 ms-spaced frames. Dual-backend plan replaced by one hyper backend on 2026-09-13, ADR 0013 item 4. | closed |
| PV-027 | Dependabot versus Renovate | CI section 6 | Dependabot weekly grouped Cargo/Actions updates. | closed |
| PV-028 | Windows mapped IPv6 resolution | Security section 8 | Normalize mapped addresses before checks. macOS and Windows run 34798946529 returned separate IPv6/IPv4 loopbacks; retain normalization. | closed |
| PV-029 | Install toolchain 1.98.1 | Toolchain section 1 | Installed aarch64 Apple toolchain selected by repository file; all gates ran on it. | closed |
| PV-030 | Modern MCP MRTR field definitions | MCP sections 2.2.2/5 | Fixed from official schema on 2026-09-14: resultType complete/input_required, keyed message/roots/elicit input requests plus requestState, retry with keyed `inputResponses`/state. Implemented elicitation only. | closed |
| PV-031 | Handwritten provider fixtures versus real responses | Testing section 10 and all four provider guides | Fixtures follow official schemas. record-fixture exists since 2026-09-14 with scenario JSON; rerecord using real credentials and compare snapshots. | open |

## Environment records

- Design machine: macOS Darwin 25.2.0 arm64, eight cores, 16 GiB, rustup 1.29.1; default stable 1.98.0 plus installed 1.98.1 with Clippy/rustfmt/rust-src/LLVM tools.
- Installed through cargo-binstall on 2026-09-13: nextest 0.9.144, deny 0.20.2, shear 1.13.4, insta 1.48.0, hack 0.6.45, semver-checks 0.50.0, llvm-cov 0.9.1, typos 1.50.1, just 1.58.0, git-cliff 2.14.1, binstall 1.23.0; release-plz 0.3.165 installed but unused.
- Skeleton gates on 2026-09-13: check/Clippy/docs with denied warnings, four deny checks, 50 feature groups, one nextest, and `typos` passed. Shear reported 292 expected skeleton declarations, then nonblocking.
- Crates.io checked 2026-09-13; local rsproxy sparse mirror resolved matching lockfile versions (395 packages in this record).
- External sources: MCP 2026-07-28 specification accessed 2026-09-13; OpenTelemetry GenAI conventions repository accessed 2026-09-14.
