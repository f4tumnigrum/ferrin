# 0010: MCP protocol implementation

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0010-mcp-protocol-implementation.md)

- Status: accepted
- Date: 2026-09-13
- Related: [MCP integration](../01-architecture/15-mcp.md)

## Context

[Fact] MCP needs JSON-RPC, HTTP/SSE, version negotiation, OAuth, elicitation, Apps, and header binding under 2026-07-28/2025-11-25 specifications.

[Fact] Official Rust SDK `rmcp` supplies protocol/client/transports; latest crates.io version at design time, 2026-09-13, was 3.3.0.

## Decision

Implement the required subset, three transports, and OAuth in `ferrin-mcp` without `rmcp`.

## Rationale

- The subset is bounded and tightly integrated with Ferrin schemas, outputs, approvals, and fingerprints.
- Redirects, expiry, resumption, header binding, and discovery need full transport control.
- Avoid public third-party SDK types and version coupling.

## Alternatives

- Wrapping `rmcp` would inherit updates but need substantial adapters and accept upstream transport constraints.

## Consequences

- Track specification updates in `protocol/versions.rs`.
- [Fact] PV-017 confirms modern removal of sessions/GET/resumption/server requests in favor of metadata/headers/MRTR, while legacy retains them. Precise dual-generation control strengthens the local implementation rationale ([MCP](../01-architecture/15-mcp.md), section 2.2.1).
- Verify subprocess/Windows behavior independently; PV-018 is now closed by Windows CI.
