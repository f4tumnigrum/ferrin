# 0005: Streaming result delivery

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0005-stream-result-delivery.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Generation loop](../01-architecture/07-generation-loop-and-streaming.md), section 3

## Context

[Fact] JavaScript streaming SDKs commonly return synchronous handles with tee views, implicit consumption, and in-stream provider errors, relying on unbounded buffering and background driving.

Rust streams are pull-based and single-consumer; multiple views need buffering or tasks.

## Decision

1. Await first request establishment before returning `Result<StreamTextResult, Error>`.
2. Provide one event stream plus `Completion`, splittable; text/partial views consume the same underlying stream.
3. No built-in multiplexing or unbounded buffers; unconsumed streams do not advance.
4. Later errors emit `StreamEvent::Error` and fail `Completion`.

## Rationale

- Clear backpressure/ownership without hidden unbounded memory or tasks.
- Handle configuration/authentication with ? at startup, following Rust HTTP usage.
- Applications explicitly pay for fan-out through broadcast when needed.

## Alternatives

- Full tee emulation needs shared queues and multiple cursors, adding complexity/leak risk.
- Background driving plus `watch` introduces implicit task behavior.

## Consequences

- Document that callers must consume events or call `consume()`.
- [Decision] PV-007: simulated streaming inherently waits for complete non-streaming generation; document that behavior without start_eager ([Generation loop](../01-architecture/07-generation-loop-and-streaming.md), section 5).
