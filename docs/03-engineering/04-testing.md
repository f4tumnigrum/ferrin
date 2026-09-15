# Testing standards

**English** | [Chinese](../zh-CN/03-engineering/04-testing.md)

## 1. Test layers

| Layer | Location | Tools | Network |
| --- | --- | --- | --- |
| Unit | tests/suite aggregated by `tests/all.rs`; see section 10 | nextest, `pretty_assertions`, `proptest` | No |
| Integration | tests/suite aggregated by `tests/all.rs` | `wiremock`, `ferrin-testing`, `insta` | Local mocks only |
| Fixture replay | tests/suite and tests/fixtures | FixtureServer | No external network |
| Contracts | StreamContractChecker | Event-order assertions | No |
| Documentation | rustdoc examples | `cargo test --doc` | No; use `MockLanguageModel` |
| Live | `tests/suite/live_*.rs`, ignored | Real APIs and environment credentials | Yes |
| Benchmarks | `benches/<name>.rs`, harness false; 11 targets in section 11 | `criterion` 0.8.2, `just bench [filter]` | Mocks/local fixtures only |
| Compile-fail | crates/ferrin/tests/ui | `trybuild` | No |

[Decision] Replay recorded raw SSE chunks and JSON, snapshotting normalized output. Gate live tests with ignore and key variables. Original provider boundaries/shapes expose parser defects better than handwritten normalized events; default runs need neither external networking nor credentials.

## 2. Running tests

- Use `just test [-p <crate>]`, nextest workspace/no-fail-fast, with an 8 MiB minimum stack.
- Nextest `default`/`local`/`ci` profiles disable retries, use 60 s slow-test periods with one-period termination, and disable fail-fast. CI adds JUnit and immediate-final failure output; live tests get 300 s.
- [Decision] Put `trybuild` in the facade, not macros: generated `ferrin::tool` paths would otherwise create a dev dependency cycle unresolved during publishing of unpublished same-version crates.
- [Decision] Since first CI on 2026-09-14, ignore `tool_macro_ui` on Windows: cold trybuild exceeded 360 s in run 34797869442 while 653 other tests passed. Platform-independent macro diagnostics remain covered on Linux/macOS.
- [Fact] One test in `crates/ferrin/tests/suite/ui.rs` drives pass/fail cases; commit stderr snapshots and refresh with `TRYBUILD=overwrite`. Cold builds create target/tests/`trybuild` projects; the test has a 180 s slow-test period rather than 60 s.
- Run ignored live tests explicitly with provider credentials; use the live-test selector documented in AGENTS.md.
- [Fact] Existing live suites cover facade generation/streaming/tool round trips/structured output, OpenAI Responses/Chat generation/streaming/tool calls, and compatible Chat. They accept `OPENAI_API_KEY`, optional base URL/model (`gpt-5` default)/provider JSON options, or compatible-specific URL/key/model variables. All passed against a third-party endpoint on 2026-09-14; it required openai store false because `item_reference` was unsupported (OpenAI guide).

[Decision] Never retry flaky tests automatically; fix nondeterministic SDK behavior.

## 3. Fixtures

### 3.1 Directories and files

```
crates/providers/ferrin-openai/tests/fixtures/
  responses/
    text-basic.request.json         # recorded request body (secrets stripped)
    text-basic.response.json        # non-streaming response body
    text-basic.chunks.txt           # streaming: one SSE event per line, exactly as received
    text-basic.meta.json            # status code, response headers (allow-listed), recorded_at, model id
    tool-call.chunks.txt
    reasoning.chunks.txt
    error-429.response.json
```

### 3.2 Recording

`cargo xtask record-fixture --provider openai --case responses/tool-call`:

1. Read scenario JSON with method/path/headers/body/stream/model. [Decision] JSON replaced the earlier scenario.rs/TOML draft on 2026-09-14; see [Workspace layout](02-workspace-layout.md), section 6.
2. Make one authenticated request through `RecordingTransport`, capturing request/body/headers and splitting SSE into chunks files.
3. Remove sensitive headers through an allowlist retaining content type, rate limits, and request IDs.
4. Write files and date/provider/case/model metadata only after scanning for secret patterns and the actual key; fail on matches.

Never hand-edit recorded fixtures; rerecord behavior changes and explain them in the PR.

### 3.3 Replay

`FixtureServer` reconstructs JSON and SSE responses; the original dual-backend design used `wiremock` for JSON and a small hyper server for delayed SSE frames (PV-026), superseded by section 10. Assert:

- Insta JSON request snapshots match recorded requests.
- Normalized `GenerateResult` or event sequence snapshots.
- Warning sets.

## 4. Core tests

- Script multi-step `MockLanguageModel` responses for every continuation branch: approval, missing executors, deferred results, stop conditions, and other finish reasons.
- Test pipeline stages independently with simulated events: tool injection ordering, ID remapping, discarded retries, stop gates.
- Pause/advance Tokio time for backoff and retry-header precedence.
- Cancel at different stages and assert Cancelled plus empty `JoinSet` cleanup.
- Cover approval signing/verification, tampering, missing calls, and policy re-resolution.
- Test complete/partial parsing for all five output strategies and JSON repair properties.

## 5. Property tests

Proptest covers:

- Every valid-JSON prefix repairs to parseable JSON.
- Arbitrary byte splitting leaves SSE decoding unchanged.
- Usage addition associativity and `None` identity.
- Reversible tool-name mapping.

## 6. Snapshots

- Use `insta` with snapshots beside tests.
- Review with `cargo insta review`; CI sets `INSTA_UPDATE=no` and rejects unaccepted snapshots.
- Inject sequential IDs and fixed clocks; exclude nondeterministic timestamps/IDs.

## 7. Coverage

- `coverage.yml` generates lcov through cargo llvm-cov nextest, writes a summary, and retains `lcov.info` for 14 days.
- [Decision] Since 2026-09-14, keep coverage in GitHub without external services such as Codecov, avoiding source-level data uploads and extra tokens. Earlier uploads silently failed without credentials. Compare local lcov with main artifacts for PR differences.
- Targets: spec/schema/core ≥85% lines, providers ≥75%; below-target coverage does not block merges.

## 8. Test data and secrets

- No real keys in the repository; recording scans secret patterns before writing.
- Credentialed CI live runs use workflow secrets in the dedicated live workflow.

## 9. Verification items

- [Fact] (PV-026, `verification/pv026-sse-server`) `wiremock` 0.6.5 supports whole bodies/delays, not chunk streaming. A `hyper` 1.11/StreamBody prototype delivered three independent frames at 0/53/105 ms with configured 50 ms intervals.
- [Decision] Original `FixtureServer` design used `wiremock` for JSON and an approximately 80-line hyper SSE server with shared formats/assertions. Superseded by the single backend in section 10 and [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md).

## 10. Implementation record (2026-09-13)

- [Decision] Tests live in tests/suite and aggregate through `tests/all.rs` and suite/mod.rs. Keep implementation files free of tests; reach internal behavior through public APIs where possible, without separately testing unreachable helpers. This keeps review and line counts focused on implementation.
- [Decision] [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 4: one hyper backend replays FixtureBody Complete/Sse with `mount`/`mount_once`/`mount_times`/`mount_file`, received requests, and chunk delays. `ferrin-testing` has no `wiremock` dependency; provider tests may still use `wiremock` directly.
- [Fact] MockLanguageModel records all/`generate`/stream calls and offers `generate`/error/repeat/with and corresponding stream scripts. `SimulatedStream` configures initial/chunk delay and hanging at end. `RecordingTransport` allowlists headers and redacts secret patterns before writing.
- [Fact] Modality tests implement specification traits inline for embeddings/images/speech/transcription/reranking/video/files/skills/batches/realtime. Local realtime servers on loopback echo WebSocket subprotocols.
- [Fact] Fixture::load prefers response.json over chunks.txt for the same `case`. OpenAI therefore uses separate `-stream` `case` names; interpret section 3.1 accordingly.
- [Pending verification] (PV-031) All four provider fixture sets were handwritten from official response schemas before `record-fixture` existed on 2026-09-14. They contain no real request IDs/accounts and have not been rerecorded with credentials. The no-hand-edit rule applies once recorded versions replace them.

## 11. Implementation record (2026-09-14, benchmarks)

- [Decision] Criterion 0.8.2 is a workspace dev dependency with `async_tokio`/`html_reports`. Each harness-free bench permits test-style unwrap/expect. Criterion provides confidence intervals, throughput, HTML, and direct Tokio timing.
- [Decision] Benchmarks use no external network or `sleep`. Core uses repeatable mocks (two-step tools alternate via atomic counters); providers reset and remount local fixtures each iteration to bound request logs. Facade synthesizes Responses SSE locally rather than referencing another crate's fixture, preserving package self-containment; concurrency uses `JoinSet`. Measure Ferrin overhead without provider latency noise.
- [Fact] Eleven targets: provider-util SSE (2000 events, whole/4096/512/64-byte feeds and decode_stream); `schema` partial JSON (25/50/75/100% of ~4 KiB, complete-parse baseline) and `schema` derivation/strict/typed/raw validation; message pruning (`none`/reasoning/all tools/before-last-4 across 40/200 `messages`); tool fingerprints (40-property canonical JSON, 5/20 tools, 20-tool drift); core generation (single/history 11/51/two-step tools/reasoning middleware) and streaming (100/1000 deltas, events/consume/smoothing); OpenAI Responses, Anthropic Messages, Google generateContent fixture generation/two streams; facade end-to-end (20/200 deltas, concurrency 1/16/64, `openai` feature).
- [Fact] Smoke benchmark run on macOS/Rust 1.98.1, 2026-09-14, warmup 0.5 s, measurement 1 s, sample size 10: all targets completed; these are not formal measurements. SSE 2000 events ~1.4–1.5 ms; JSON repair 2.7–11 µs; 200-message pruning 15–35 µs; one/two-step generation ~7/39 µs; 1000 deltas ~0.7 ms; adapter generation ~60 µs and streams 90–180 µs including local HTTP; end-to-end 200 deltas ~1.6 ms and 64 concurrent 50-delta streams ~8.3 ms.
- [Decision] Do not commit results or gate CI timing. Clippy all-targets compiles benches; manual `bench.yml` accepts a `filter` and retains criterion artifacts 30 days. Compare saved baselines on the same machine because shared runners are noisy.
- [Fact] Workspace-wide criterion flags fail on libtest harnesses without bench declarations (sample-size rejected by `ferrin-spec` on 2026-09-14). Both accept name filters, so just bench passes only a filter; target-specific criterion options use `cargo bench -p <crate> --bench <name> -- --save-baseline <tag>`.

[Fact] `StreamContractChecker` remembers text, reasoning and tool-input IDs after their end events and rejects reuse within one provider call. Independent checker instances allow reuse across calls. Regression: `closed_part_ids_cannot_be_reused` (2026-09-15, review I02).

[Fact] `FixtureServer::mount_times(..., 0)` mounts no route, so the next matching request reaches the fallback or returns 404. Regression coverage checks zero, one and multiple responses (2026-09-15, review I03).
