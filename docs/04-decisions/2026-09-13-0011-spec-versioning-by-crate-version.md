# 0011: Specification versioning through crate versions

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0011-spec-versioning-by-crate-version.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Provider specification](../01-architecture/04-provider-spec.md), section 1; [Versioning](../03-engineering/06-versioning-and-release.md), section 8

## Context

[Fact] Dynamic SDKs need runtime version upgrades for adapter coexistence; Rust checks changed traits at compile time and Cargo SemVer already expresses compatibility.

## Decision

1. No runtime specification fields or upgrade adapters.
2. Breaking spec releases are specification upgrades, bound through Cargo; `SPEC_VERSION` is diagnostic only.
3. Release all crates at one version during `0.y`.

## Rationale

- Compile-time checks enforce compatible contracts without runtime version markers.
- The core needs no multi-version branches.

## Alternatives

- spec_version and upgrade adapters add complexity already handled by Cargo constraints.

## Consequences

- Update all first-party adapters together; third-party adapters must follow upgrades.
- Specification changes need two approvals and an ADR.
