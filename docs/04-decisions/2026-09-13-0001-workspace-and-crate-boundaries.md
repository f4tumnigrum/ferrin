# 0001: Workspace and crate boundaries

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0001-workspace-and-crate-boundaries.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Crate boundaries](../01-architecture/02-crates.md)

## Context

[Fact] Major provider requests, stream events, and errors differ, while applications need one interface. MCP tools require definitions/execution, not generation-loop types.

[Fact] Cargo workspaces share lockfiles/lints while allowing independent publication. A monolithic crate adds unused-provider/modality compile cost and encourages an oversized core.

## Decision

Use six layers: spec L0; schema/message L1; provider utilities/tool L2; providers/MCP L3; core L4; facade/OTel/testing/macros L5. Dependencies flow down; providers and MCP do not depend on core.

## Rationale

- Minimize adapter/MCP dependencies and permit independent third-party adapter releases.
- Separate application messages/tools from pure serializable specification data.
- Give applications one facade dependency with provider features.

## Alternatives

- Monolith with features: growing compile/dependency costs and hard-to-test combinations.
- Per-modality core crates: duplicate retries/telemetry/conversion or require another shared layer for little gain.

## Consequences

- Maintain topological publication through `xtask publish-order`.
- Define cross-crate types in spec/L1, requiring upfront design.
