# 0007: Tokio runtime and cancellation

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0007-tokio-runtime-and-cancellation.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Concurrency and cancellation](../01-architecture/16-concurrency-and-cancellation.md)

## Context

[Fact] Generation combines caller cancellation and scoped timeouts across models/downloads/tools; concurrency and streaming aggregation require tasks/timers.

[Fact] Tokio underpins reqwest, hyper, WebSockets, and major MCP implementations; Clippy can reject locks held across await.

## Decision

1. Support only Tokio, without implicit runtimes or `block_on`.
2. Derive CancellationToken scopes from call to step to model/tool/download.
3. Own tasks through `JoinSet`; ban bare spawn.
4. Public futures/streams are `Send`.

## Rationale

- Runtime abstraction adds maintenance while major dependencies still require Tokio.
- Hierarchical tokens express caller-or-timeout cancellation.
- `JoinSet` prevents orphan tasks.

## Alternatives

- `async-std`/`smol` compatibility would require timer/task abstractions with little benefit.
- AbortHandle lacks hierarchical timeout scopes.

## Consequences

- Applications must call Ferrin within Tokio.
- Track cancellation cause separately to distinguish timeouts.
