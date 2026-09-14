# 0009: HTTP transport and secure URLs

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0009-http-transport-and-secure-url.md)

- Status: accepted
- Date: 2026-09-13
- Related: [HTTP security](../01-architecture/14-http-and-security.md), [Security practices](../03-engineering/08-security-practices.md)

## Context

[Fact] File downloads and MCP endpoints expose SSRF risk unless HTTPS, private/local rejection, redirect validation, DNS pinning, size limits, and audited entry points are enforced.

[Fact] Injectable transport traits enable replay tests and application proxies/custom environments.

## Decision

1. `HttpTransport` defaults to reqwest 0.13.5/rustls with automatic redirects disabled.
2. Implement SSE decoding locally.
3. Implement `UrlPolicy`/`validate_url`/`fetch` with all-address validation, pinning, manual redirects, and size limits.
4. Ban direct reqwest/environment reads outside audited modules through Clippy.

## Rationale

- Traits support test/proxy/runtime substitution.
- Local SSE supports timestamps/limits without stale dependencies.
- Compile-time enforcement is stronger than review conventions alone.

## Alternatives

- Direct reqwest clients couple public APIs and complicate replacement/testing.
- Existing EventSource wrappers lack needed timing/limit hooks.

## Consequences

- [Fact] PV-015 verified reqwest pinning/no-redirect APIs and ~58 µs client construction; `rustls` is the 0.13 feature and system verification is default.
- Providers send only through provider utility HTTP.
