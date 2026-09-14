# 0015: Serialize MCP stdio frames through a writer task

**English** | [Chinese](../zh-CN/04-decisions/2026-09-14-0015-mcp-stdio-frame-writer.md)

- Status: accepted
- Date: 2026-09-14
- Related: [MCP](../01-architecture/15-mcp.md), sections 5/6; [Coding standards](../03-engineering/03-coding-standards.md); Clippy invalid await-held types

## Context

The original baseline locked stdin for atomic JSON-line writes. Clippy rejects Tokio mutex guards across write_all/flush awaits, and standard guards cannot wrap async writing. Record the revision under the [ADR process](../03-engineering/07-adr-process.md).

## Decision

1. One `JoinSet`-owned writer owns `ChildStdin`. send queues complete frames and `oneshot` receipts through unbounded `mpsc`; the writer writes/flushes in order and returns I/O results.
2. Frames never interleave; send waits for its receipt and maps failures to `McpError::Io`.
3. Closing drops the sender, allowing drain and stdin shutdown, then starts process kill and waits. Reader EOF also closes the channel and emits Closed.
4. Keep kill-on-drop, Windows no-console flags, and newline rejection; Windows CI validates PV-018.

## Rationale

- No guard crosses `await`, and caller cancellation cannot interrupt a half-written frame because the writer owns it.
- Receipts preserve write-result reporting; queue insertion is nonblocking, with upper-level request timeouts bounding waits.

## Alternatives

- Reject locked Tokio stdin because lints prohibit it.
- Reject synchronous standard stdin because it blocks runtime threads and mixes process APIs.
- Reject try-lock spinning with `BytesMut` because a guard still crosses `await`.

## Consequences

- Update MCP baseline and implementation records.
- The pattern can also serve future multi-producer async byte streams such as WebSocket writers.
