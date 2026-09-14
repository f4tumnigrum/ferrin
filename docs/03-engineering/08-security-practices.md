# Security practices

**English** | [Chinese](../zh-CN/03-engineering/08-security-practices.md)

## 1. Threat model

Ferrin processes untrusted input from:

| Source | Content | Risk |
| --- | --- | --- |
| Model output | Tool arguments, URLs, JSON | SSRF and resource exhaustion from oversized/malformed JSON |
| Provider responses | Bodies, SSE, headers | Parse failures or memory growth |
| Persisted history | Approvals, tool results | Tampering to bypass policy |
| MCP servers | Definitions, results, elicitation | Definition drift, application-handled prompt injection, malicious resources |
| Environment/configuration | Keys, base URLs | Leakage or malicious endpoints |

## 2. Network access

[Decision] Downloads and MCP endpoints follow HTTPS-only defaults, private/local rejection, per-hop validation, DNS pinning, 100 MiB download limits, and audited transport entry points enforced by Clippy. These jointly prevent SSRF/rebinding; see [HTTP security](../01-architecture/14-http-and-security.md).

- Route outbound requests through provider utility HTTP; Clippy blocks direct reqwest calls.
- Model/message URLs download only through `secure_url::fetch`; see HTTP section 8.
- Provider base URLs come from explicit config/environment; validate absolute HTTP/HTTPS without credentials. Explicit HTTP supports local proxies and warns.
- Do not follow redirects automatically.
- The original TLS baseline used rustls/webpki-roots with optional system verification; the implemented reqwest 0.13 system-verifier configuration is recorded in the HTTP chapter.

## 3. Secrets

- API keys, approval secrets, and OAuth tokens use `secrecy` with redacted `Debug`/`Display`.
- Expose only when building authenticated requests; never log or store secrets in errors, telemetry, or fixtures.
- Redact Authorization, API key, Cookie, Set-Cookie, Proxy-Authorization, and provider-registered sensitive headers.
- Fixture recording allowlists response headers and scans secret patterns.
- Centralize environment reads in settings for auditability.

## 4. Tool execution

- Validate inputs before execution; dynamic MCP schemas use JSON Schema validation when enabled.
- Approval-required tools do not execute by default. Optional HMAC-SHA256 signatures use constant-time verification; revalidate input and policy ([Tool system](../01-architecture/06-tool-system.md)).
- Applications may compare fingerprints across requests for definition drift.
- Timeouts/cancellation constrain execution; nonfatal tool errors return to the model.
- LocalProcessSandbox explicitly provides no isolation and is test-only infrastructure.

## 5. Resource limits

| Resource | Default limit | Location |
| --- | --- | --- |
| Downloads | 100 MiB | `UrlPolicy::max_body_bytes` |
| Non-streaming response, original design limit | 64 MiB | HTTP limits; implemented handler defaults are recorded in the HTTP chapter |
| SSE event | 16 MiB | `SseDecoder` |
| JSON nesting | 128 | `ferrin_schema::json` |
| Tool input JSON | 4 MiB | `parse_tool_call` |
| Tool result channel | 64 items | Streaming pipeline |

On excess, abort the current request and return non-retryable API/JSON parsing errors.

[Decision] Limits are constants overridable through builder limits, never implicit environment configuration.

## 6. Dependency security

- Deny advisories run on pushes/PRs and weekly issue checks; enable Dependabot alerts/security updates ([CI](05-ci-and-quality-gates.md), section 8). Patch high-severity vulnerabilities within 72 hours.
- `unsafe_code = "forbid"`.
- Dependencies come only from crates.io.
- CI builds releases from tags; maintainers do not publish locally, and registry tokens remain in CI.

## 7. Vulnerability reporting

- `SECURITY.md` documents private reporting, acknowledgement within three business days, and a 30-day remediation target.
- After release, document `Security` changes and request a RUSTSEC advisory if downstream users are affected.

## 8. Verification items

- [Fact] (PV-022, `verification/pv022-crypto`) `hmac` 0.13/`sha2` 0.11/`subtle` 2.6/`secrecy` 0.10 compile together. Import KeyInit for Hmac::new_from_slice, store keys as `SecretBox<[u8]>`, and expose for signing. `Mac::verify_slice` is constant-time; precomputed digest comparisons use ConstantTimeEq::ct_eq.
- [Decision] (PV-028) Normalize mapped IPv6 to IPv4 before private checks with to_ipv4_mapped, independent of resolver representation. [Fact] macOS localhost resolves to IPv6/IPv4 loopback. Windows CI run 34798946529 on 2026-09-14 recorded the same pair without mapped addresses, closing PV-028. Retain normalization because other resolver configurations may return mapped forms.
