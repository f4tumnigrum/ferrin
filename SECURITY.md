# Security Policy

## Reporting a vulnerability

Report suspected vulnerabilities privately through the repository's security
advisory form ("Report a vulnerability" under the Security tab). Do not open a
public issue. Expect an acknowledgement within 3 business days.

## Scope

- Credential handling (`secrecy`-wrapped keys, header redaction in logs).
- Secure URL policy for downloads and MCP endpoints (HTTPS only, private
  network rejection, DNS pinning, size limits).
- Tool approval signatures (HMAC-SHA256) and replay validation.
- Dependency advisories (`cargo deny` runs in CI on every push and pull
  request and weekly in `versions.yml`; Dependabot alerts and security
  updates are enabled).

See `docs/03-engineering/08-security-practices.md` for the full practices.

## Supported versions

Fixes are released for the latest minor version; security fixes may also be
backported to the previous minor version.
