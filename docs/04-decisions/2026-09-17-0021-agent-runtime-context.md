# 0021: Persistent step state and separate runtime context

**English** | [Chinese](../zh-CN/04-decisions/2026-09-17-0021-agent-runtime-context.md)

- Status: proposed
- Date: 2026-09-17
- Related: [Generation loop](../01-architecture/07-generation-loop-and-streaming.md), [Agent](../01-architecture/09-agent.md), [Tool system](../01-architecture/06-tool-system.md)

## Context

[Fact] Ferrin's shared step preparation reconstructs messages, instructions and tools context from the initial call on every step (`crates/ferrin-core/src/generate_text/inputs.rs`, inspected 2026-09-17). The local Vercel AI SDK reference carries step overrides forward and supplies separate `runtimeContext` and `toolsContext` (`packages/ai/src/generate-text/prepare-step.ts`).

## Decision

[Decision] Give each generation invocation private evolving state for messages, instructions, tools context and runtime context. A `prepare_step` override replaces the corresponding state for this and subsequent steps. Append only responses produced after the latest message override. Keep initial messages and complete response history separately available to the callback. Model, tool selection/order and sampling overrides remain local to one step.

[Decision] Expose `runtime_context: Option<JsonValue>` on call and agent builders, prepared agent calls, step preparation, approval contexts, lifecycle hooks and step results. It is application state, never a provider parameter or tool execution context. `None` means unchanged in overrides; JSON `null` is an explicit replacement value. Capture both contexts on each step; deserialize older results without them. Approval replay uses the new invocation's initial contexts.

[Decision] Telemetry integration copies include runtime context only with `include_runtime_context`, and tool context only with `include_tools_context`; both options default to false. Application hooks and returned results retain their contexts.

[Decision] Copy tool-definition metadata to parsed calls and tool outcomes, including invalid calls, provider results and streamed preliminary outcomes. Keep provider metadata separate. Missing fields in persisted results deserialize as `None`.

## Rationale and alternatives

[Decision] JSON runtime state follows Ferrin's existing serializable context conventions without adding generics throughout the public generation API. Independent per-invocation state avoids cross-call leakage from reusable agents. Retaining the complete response history preserves audit output after prompt compression.

[Decision] Resetting overrides every step forces callbacks to implement their own persistence and can restore messages deliberately removed for compression. Passing runtime state to executors would mix application lifecycle data with schema-validated tool input context.

## Consequences

[Decision] This changes continuation semantics and adds fields to public structs and stream events, requiring updates to downstream Rust struct literals. The implementation remains an unreleased proposal pending maintainer review; this document does not assert acceptance. No provider protocol or external dependency changes are needed.
