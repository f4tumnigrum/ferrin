# 0012: Tool typing strategy

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0012-tool-typing-strategy.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Tool system](../01-architecture/06-tool-system.md), section 1.1

## Context

[Fact] TypeScript mapped/union types derive tool-set input/output types at no runtime cost; Rust needs per-set enums or macros.

Rust alternatives require generated enums or `Any`-based trait-object downcasts.

## Decision

1. Type definitions with `Tool::function::<I>()`, DeserializeOwned/JsonSchema input, and serializable output.
2. Carry calls/results as JSON in steps/events, with typed extraction helpers.
3. The optional tool macro generates definitions without changing runtime representation.

## Rationale

- Validate/serialize at definition boundaries.
- Avoid per-set enum compilation/API complexity.
- Share one representation with dynamic MCP tools.

## Alternatives

- Generated tool-set enums make merging difficult.
- Any outputs risk downcast failure and cannot serialize.

## Consequences

- Callers specify extraction types; mismatches return errors at runtime.
- `ToolSet` freely merges local and remote tools.
