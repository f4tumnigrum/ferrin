# 0008: No implicit default provider

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0008-no-implicit-default-provider.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Middleware and registry](../01-architecture/10-middleware-and-registry.md), section 2.3

## Context

[Fact] Some SDKs resolve string models through a process-global provider defaulting to a hosted gateway, making unconfigured network calls and relying on global mutable state.

## Decision

1. No built-in gateway/default provider; network access requires explicit configuration.
2. Resolve strings through caller registries or a once-installed default registry; otherwise return NoDefaultRegistry.

## Rationale

- Hosted gateways are outside scope.
- Explicit network destinations make data flow auditable.
- `OnceLock` prevents runtime races/configuration drift.

## Alternatives

- Environment-selected defaults would make behavior implicit.

## Consequences

- Examples obtain models from explicitly created providers.
