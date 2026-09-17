# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Include the generation step's independent `runtime_context` in default approval
  policy input, alongside `tools_context` (ADR 0021).

### Changed

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

## [0.1.2] - 2026-09-16

### Fixed

- Enforce capability restrictions through the core execution boundary, including streamed calls.
- Omit policy reasons and error payloads from logs and redact server URL credentials in diagnostic output.

### Added

- `PolicyClient` trait with `policy_client` closure adapter, `PolicyDecision`
  normalization of OPA-style decision documents (`decision` objects, legacy
  `allow` booleans, bare booleans, `null` as not applicable, everything else
  denied) and `PolicyError`.
- `policy_approval(client, path)`: an `ApprovalPolicy` that evaluates the
  policy with the tool call, input, messages and tools context as input;
  `to_input` override and `FailureMode` (deny by default, fall through on
  request).
- `shadow(policy)` observes decisions without enforcing them until switched
  to `Enforcement::Enforce`; `with_default(policy, status)` gives calls the
  policy does not cover a fixed status.
- `capability_middleware(client, path)`: a `LanguageModelMiddleware` that
  restricts the tools offered to the model to the policy's allowlist and
  clears them when evaluation fails.
- `HttpPolicyClient` for the OPA REST Data API (`POST /v1/data/<path>`)
  through `ferrin_provider_util::http` with the secure URL policy.
- Feature `rego`: `RegoPolicyClient` evaluates Rego policies in-process with
  `regorus`.
