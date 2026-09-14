# 0016: Inline encoding without spawn_blocking

**English** | [Chinese](../zh-CN/04-decisions/2026-09-14-0016-inline-encoding-no-spawn-blocking.md)

- Status: accepted
- Date: 2026-09-14
- Related: [Concurrency](../01-architecture/16-concurrency-and-cancellation.md), section 7; [Coding standards](../03-engineering/03-coding-standards.md); core limits

## Context

The original policy sent base64/JSON over 1 MiB to spawn_blocking and reserved an unused BLOCKING_ENCODE_THRESHOLD_BYTES constant.

[Fact] After all 15 crates were implemented on 2026-09-14, no `spawn_blocking`/`block_in_place` call existed; conversion, modalities, and provider requests encoded inline, and the threshold was unread. Revise the decision through the [ADR process](../03-engineering/07-adr-process.md) instead of adding an unneeded pool path.

## Decision

1. Encode base64/JSON and pure CPU transforms in caller async tasks, without core/provider blocking-pool calls.
2. Remove the unused threshold; retain only used capacity/concurrency constants.
3. Update concurrency documentation. Future measurable megabyte-scale runtime blocking requires a new ADR and benchmarks before adding thresholds/pool paths.

## Rationale

- Most input files are small and downloads are bounded; millisecond-scale encoding is below network latency, without enough benefit to justify pool saturation, `JoinError`, and cancellation complexity.
- Running blocking tasks cannot be interrupted by caller tokens; inline work follows the surrounding await-boundary cancellation model.
- `JoinSet` ownership already governs tasks; avoid a second lifecycle model for blocking work.

## Alternatives

- Reject adding blocking calls merely to use the constant without measured need.
- Reject `block_in_place` because current-thread runtimes used by `xtask`/tests would panic.

## Consequences

- Core limits shrinks to three constants and the obsolete threshold prose is removed.
- Future blocking paths need ADRs, measurements, and documented cancellation semantics.
