# 0013: Core implementation revisions

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0013-core-implementation-revisions.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Agent](../01-architecture/09-agent.md), sections 2/5; [Modalities](../01-architecture/11-other-modalities.md), section 9; [Observability](../01-architecture/13-observability.md), section 3; [Testing](../03-engineering/04-testing.md), section 9; PV-009/PV-026

## Context

Implementation required five revisions for compiler constraints/API consistency. This ADR records them under the [ADR process](../03-engineering/07-adr-process.md), section 6; corresponding chapters carry dated revisions.

## Decision

1. Agent options require only Send/static, not DeserializeOwned/JsonSchema; call_options changes the generic type.
2. `PreparedCall` uses populated ordinary fields instead of Override. The callback receives merged effective `defaults` and removes settings by assigning `None`.
3. Realtime events yield `Result<RealtimeServerEvent, Error>`; register tools on the builder before connect so the initial session update includes definitions.
4. `FixtureServer` uses one hyper backend for JSON/SSE, without `wiremock`.
5. Modalities share `ferrin.modality`, distinguished by `gen_ai.operation.name`, rather than per-modality span names.

## Rationale

1. Statically constructed `options` have no deserialization boundary needing schema validation; requiring derives produced unused schemas.
2. Once effective defaults are merged, leaving fields unchanged means keep and `None` means clear. Override becomes redundant and duplicates `CallSettings` field types. PV-009 compared nested Options without considering premerged defaults.
3. `Result` items distinguish local transport/tool failures from provider `Error` events and normal closure. Tools must exist before the first session update.
4. Fixtures share recording/assertions/delays; one backend provides one mount API/listener. Wiremock lacks streaming, adding a second matcher/dependency solely for JSON.
5. Tracing span names must be static literals; operation fields avoid repetitive macros and follow GenAI conventions.

## Alternatives

- Keeping Override requires redundant conversions and meaningless Keep states after merging.
- Plain realtime events plus separate error callbacks add another callback channel and diverge from stream error delivery.
- Dual fixture backends have the costs described above.
- Per-modality names add implementation cost and excessive sibling span names.

## Consequences

- Added revision notes to [Agent](../01-architecture/09-agent.md), [Modalities](../01-architecture/11-other-modalities.md), [Observability](../01-architecture/13-observability.md), and [Testing](../03-engineering/04-testing.md).
- PV-009 now records supersession by item 2; PV-026 records the single backend.
- Retain the override prototype as history, no longer matching implementation.
