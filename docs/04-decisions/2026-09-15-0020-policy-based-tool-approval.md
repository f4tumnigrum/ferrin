# 0020: Policy-based tool approval crate and embedded Rego engine

**English** | [Chinese](../zh-CN/04-decisions/2026-09-15-0020-policy-based-tool-approval.md)

- Status: accepted
- Date: 2026-09-15
- Related: [Tool system](../01-architecture/06-tool-system.md), [Policy-based tool approval](../01-architecture/18-policy-approval.md), [ADR 0017](2026-09-14-0017-apache-2-license-and-attribution.md)

## Context

[Fact] Ferrin resolves tool approvals through the `ApprovalPolicy` trait (tool system, section 4.1); every decision so far was written in Rust in the application. Organisations that run Open Policy Agent express authorization as Rego policies evaluated from a JSON input, either through OPA's REST Data API or by embedding an evaluator.

[Fact] The Vercel AI SDK ships a policy package with HTTP and WASM policy clients, decision normalization, a shadow mode, a total-coverage wrapper for MCP tools and a capability middleware (repository read on 2026-09-15). Ferrin had no equivalent; the gap analysis of 2026-09-15 listed it.

## Decision

[Decision] Add the crate `ferrin-policy` (layer L5, depends on `ferrin-core`, `ferrin-spec`, `ferrin-provider-util`) with a `PolicyClient` trait, `PolicyDecision` normalization, `policy_approval` (an `ApprovalPolicy`), `shadow`, `with_default` and `capability_middleware`. The facade exposes it behind `policy` and `policy-rego`.

[Decision] Two clients: `HttpPolicyClient` for the OPA REST Data API through `ferrin_provider_util::http` with the secure URL policy, and `RegoPolicyClient` behind the crate feature `rego`, evaluating Rego in-process with `regorus` 0.12.

[Decision] Defaults fail closed: evaluation errors deny the call (or clear the tools), unrecognized decision documents deny, and `not-applicable` (or an undefined rule) falls through to the tool's own `needs_approval`. Fail-open behaviour is opt-in (`FailureMode::FallThrough`). Bare booleans are accepted as decisions in addition to the decision-object and legacy `allow` forms.

## Rationale and alternatives

[Decision] A separate crate rather than a `ferrin-core` module: the HTTP client, the URL policy plumbing and above all the Rego interpreter (about forty transitive crates with its default features) must not be paid for by every user of the generation loop, and the crate can evolve at its own pace.

[Decision] `regorus` rather than the alternatives: OPA's WASM bundles need a WebAssembly runtime such as `wasmtime` (a very large dependency, and policies must be precompiled with the `opa` tool); shelling out to the `opa` binary means process management and an external installation; `regorus` is a pure-Rust interpreter by Microsoft with permissive licensing (`MIT AND Apache-2.0 AND BSD-3-Clause`, all on the deny allow list), a `Clone` engine that allows lock-free concurrent evaluation, and coverage of the OPA built-ins through its `full-opa` feature.

[Decision] One crate with a feature instead of a `ferrin-policy-rego` crate: the HTTP and Rego clients share the path normalization, the decision format and the approval adapters, and the optional dependency is confined to `rego.rs`.

[Decision] Not-applicable falls through instead of approving: a policy without a rule for a tool must not override the tool author's `needs_approval`; `with_default` exists for deployments that want a total decision.

## Consequences

[Decision] The workspace has 16 crates; `ferrin-policy` publishes after `ferrin-core`, beside `ferrin-otel` and `ferrin-testing`. It starts at the workspace version 0.1.1 with an `Unreleased` changelog section and no published release yet.

[Decision] With `rego` enabled, `regorus` adds `anyhow`, `lazy_static`, `num-bigint`, `spin` and a second `jsonschema` major (0.49) to the dependency graph; these are transitive only, the crate itself uses neither `anyhow` nor `lazy_static`, and the duplicate is a `multiple-versions` warning as recorded in the toolchain document. The Windows MSVC build of the `rego` feature depends on Spectre-mitigated CRT libraries (PV-032).

[Decision] The decision format and the shadow and capability patterns derive from the Vercel AI SDK; `ferrin-policy` is added to the attribution list of the root `NOTICE` and carries the `# Attribution` section of ADR 0017.
