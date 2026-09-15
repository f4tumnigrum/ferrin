# Policy-based tool approval

**English** | [Chinese](../zh-CN/01-architecture/18-policy-approval.md)

Implemented in `ferrin-policy` ([ADR 0020](../04-decisions/2026-09-15-0020-policy-based-tool-approval.md)). The crate binds the `ApprovalPolicy` contract of [Tool system](06-tool-system.md), section 4, to policy engines that follow the Open Policy Agent conventions: a JSON input, a rule path, a JSON decision.

## 1. Scope and placement

[Decision] Approval decisions leave application code when a team wants one rule set across services, an audit trail of every decision, or a change process for rules that is separate from deployments. `ferrin-policy` evaluates such rules through a `PolicyClient` and maps the decision onto Ferrin's approval statuses; it defines no policy language of its own.

[Decision] The crate is a layer L5 crate beside `ferrin-otel`: it depends on `ferrin-core` (approval and middleware traits), `ferrin-spec` and `ferrin-provider-util` (HTTP transport, secure URL policy). The facade exposes it behind the features `policy` (`ferrin::policy`) and `policy-rego` (adds the embedded Rego engine). Rationale: the HTTP client and the Rego interpreter must not become dependencies of every `ferrin-core` user.

## 2. Policy clients

### 2.1 Interface

[Decision] A client evaluates a path with an input and returns the raw decision document; it reports `null` (not an error) when the policy produced no value.

```rust
pub trait PolicyClient: Send + Sync + 'static {
    fn evaluate<'a>(&'a self, path: &'a str, input: JsonValue)
        -> BoxFuture<'a, Result<JsonValue, PolicyError>>;
}
```

`Arc<T>` forwards to `T`; `policy_client(|path, input| ..)` adapts a synchronous closure for tests and static rules. `PolicyError` is a `thiserror` enum (`InvalidPath`, `InvalidUrl`, `InvalidInput`, `Transport`, `Status`, `InvalidResponse`, `Engine`); approval policies never surface it to the generation loop (section 4).

[Decision] Paths are accepted in the OPA REST form (`ferrin/tools/decision`) and the Rego form (`ferrin.tools.decision`), with an optional leading `/` or `data` segment. Empty segments and whitespace are rejected. Rationale: the same policy string then works with both clients.

### 2.2 HTTP client (OPA REST Data API)

[Fact] The OPA Data API evaluates a document with input through `POST /v1/data/{path}` with the body `{"input": <document>}` and answers `{"result": <value>}`; the `result` member is absent when the document is undefined (source: OPA documentation, REST API, Data API).

[Decision] `HttpPolicyClient::builder(base_url)` sends that request through `ferrin_provider_util::http` (`HttpTransport`, default shared `reqwest` transport or an injected one) with `content-type` and `accept` set to `application/json` unless the configured headers already set them, a `ferrin-policy/<version>` user-agent suffix, an optional per-request timeout, a 1 MiB response limit by default, and configured headers for authentication. `base_url` may carry a path prefix; `/v1/data/<segments>` is appended to it. A missing `result` yields `null`; non-success statuses become `PolicyError::Status` with at most 1 KiB of the body; a non-object body is `InvalidResponse`.

[Decision] The server URL is validated with a `UrlPolicy` on every evaluation, resolving and pinning addresses as in [HTTP transport and security](14-http-and-security.md). The default policy is the strict one (HTTPS, public networks); a local sidecar needs `UrlPolicy::new().allow_http().allow_private_networks()`. Rationale: the same default as MCP endpoints, so that relaxing it is an explicit decision of the operator.

### 2.3 Embedded Rego client (feature `rego`)

[Fact] `regorus` 0.12.0 (crates.io `max_stable_version` on 2026-09-15; license `MIT AND Apache-2.0 AND BSD-3-Clause`; no `rust-version` declared) provides `Engine::new()`, `add_policy(path: String, rego: String) -> Result<String>` (returns the `data.<package>` path), `add_data(Value)`, `set_input(Value)` and `eval_rule(String) -> Result<Value>`; `Engine` is `Clone`, `Value: From<serde_json::Value> + Serialize`, and an undefined rule evaluates to `Value::Undefined`. The default features are `full-opa`, `arc` and `rvm` (source: the crate source of 0.12.0).

[Decision] `RegoPolicyClient::builder().policy(name, source).data(json).build()` parses the modules and merges the data documents once. Every evaluation clones the prepared engine, sets the input and evaluates `data.<path>`, so the client is `Sync` without locks. `Value::Undefined` becomes `null` (not applicable); a rule path that does not exist is a `PolicyError::Engine` error, which the approval policy turns into a denial. Rationale: an undefined rule is the normal Rego way to say "no opinion", whereas a misspelled path is a configuration error that must not silently approve calls.

[Fact] (PV-032, verified 2026-09-16) The `test (windows-2025)` job of [CI run 35032650222](https://github.com/f4tumnigrum/ferrin/actions/runs/35032650222) passed with all features, including the Rego client, at `7f950ad`. `regorus` still requires the Spectre-mitigated CRT libraries for MSVC; this result verifies the hosted runner, not arbitrary Windows installations.

## 3. Decision documents

[Decision] `PolicyDecision::normalize(raw)` accepts the following forms and rejects everything else as a denial with the reason `unrecognized policy decision`, so that a broken or misrouted policy fails closed:

| Raw document | Decision |
| --- | --- |
| `null` (undefined rule, missing `result`) | `NotApplicable` |
| `true` / `false` | `Allow` / `Deny` |
| `{"decision": "allow" \| "deny" \| "requires-approval" \| "not-applicable", "reason"?}` | the named decision with the reason |
| `{"allow": bool, "reason"?}` (legacy form) | `Allow` / `Deny` |
| anything else, including an unknown `decision` string | `Deny` with the unrecognized reason |

[Decision] `into_approval` maps `Allow` to `Approved`, `Deny` to `Denied`, `RequiresApproval` to `UserApproval` (reasons preserved) and `NotApplicable` to `None`, which makes the approval policy fall through to the tool's own `needs_approval`. Rationale: a policy that has no rule for a tool must not override the tool author's declaration.

[Decision] The bare boolean form is a Ferrin extension: `default allow := false` followed by `allow if { .. }` is the most common Rego idiom, and its meaning is unambiguous.

## 4. Approval policy

[Decision] `policy_approval(client, path)` implements `ApprovalPolicy`. The default input is

```json
{
  "tool": { "name": "..", "tool_call_id": "..", "dynamic": false, "provider_executed": false, "invalid": false },
  "input": <parsed tool input>,
  "messages": [<messages of this step>],
  "tools_context": <tools context or null>
}
```

and `to_input(|call, ctx| ..)` replaces it, for example to drop the messages. Evaluation errors deny the call with the reason `policy evaluation failed`; `on_error(FailureMode::FallThrough)` returns `None` instead. Decisions are logged at `debug`, failures at `warn`, with the tool name, path and decision type only; reasons and error payloads are omitted.

[Decision] `with_default(policy, status)` returns `status` for calls the inner policy leaves undecided, so that tools without a `needs_approval` declaration (for example tools bridged from an MCP server) do not execute silently. `shadow(policy)` evaluates the inner policy, reports every decision through `on_decision(|call, status| ..)` and returns `None` until `enforcement(Enforcement::Enforce)` is set; a rollout switches from observing to enforcing without rewiring.

Example policy for the default input:

```rego
package ferrin.tools

import rego.v1

default decision := {"decision": "not-applicable"}

decision := {"decision": "deny", "reason": "protected path"} if {
    input.tool.name == "delete_file"
    startswith(input.input.path, "/tmp/")
}

decision := {"decision": "requires-approval"} if {
    input.tool.name == "delete_file"
    not startswith(input.input.path, "/tmp/")
}
```

## 5. Capability middleware

[Decision] `capability_middleware(client, path)` is a `LanguageModelMiddleware` whose `transform_params` restricts `CallOptions::tools` to the allowlist returned by the policy: an array of tool names or `{"tools": [..]}`. Calls without tools are not evaluated. The default input is `{"model": {"provider", "model_id"}, "call": "generate" | "stream", "tools": [{"name", "provider_defined"}], "tool_choice"}`; `to_input` replaces it. Evaluation failures and unrecognized documents remove every tool (fail closed) unless `on_error(FailureMode::FallThrough)` keeps them. A `tool_choice` that forces a removed tool, or requires a tool when none remains, is cleared so that providers do not reject the request.

## 6. Security

- [Decision] The default approval input includes the messages of the step; a policy server therefore sees prompts and tool outputs. Deployments that must not share them use `to_input` to send only the tool name and input.
- [Decision] Authentication headers of the HTTP client are configured through `Headers` and appear masked in `Debug` output; the crate logs no headers, inputs or responses.
- [Decision] All failure paths default to denial (approval) or to an empty tool list (capabilities); fail-open behaviour is opt-in through `FailureMode::FallThrough`.
- [Decision] Rego evaluation runs in-process on the caller's task; policies are operator-supplied code, and the engine's execution and policy-size limits are left at `regorus` defaults.

## 7. Verification items

- [Decision] Minimum coverage of `ferrin-policy` (`tests/suite/`): the normalization table of section 3 including every rejected form; the default input document and the `to_input` override; denial and fall-through on evaluation errors; `with_default`; `shadow` in both enforcement modes; capability filtering with both allowlist forms, fail-closed and fall-through, `tool_choice` cleanup, and no evaluation without tools; the HTTP wire format (path with either separator, body, headers, user agent, missing `result`, non-success status, non-JSON and non-object bodies, invalid paths, default URL policy rejecting a local HTTP server, base path prefix) against `FixtureServer`; the Rego client (input and data, undefined rule, missing rule, invalid policy) and its use through `policy_approval`; denied, allowed and user-approval decisions observed through `generate_text` with `MockLanguageModel`.
- [Fact] PV-032 is closed by the Windows CI build and tests recorded in section 2.3.

## 8. Implementation record (2026-09-15)

- [Fact] `crates/ferrin-policy/src/`: `client.rs`, `decision.rs`, `approval.rs` (`policy_approval`, `with_default`, `FailureMode`, `default_input`), `shadow.rs`, `capability.rs`, `http.rs`, `rego.rs` (feature `rego`), `path.rs`, `error.rs`. Tests: `tests/suite/{decision,approval,shadow,capability,http,rego}.rs`, following the list in section 7.
- [Fact] The facade re-exports the crate as `ferrin::policy` behind `policy`; `policy-rego` forwards `ferrin-policy/rego`.

[Decision] Capability filtering constrains execution and tool-choice validation through the core middleware tool contract. Diagnostics omit decision reasons, error payloads and server URL credentials; the explicit decision callback remains the application-controlled audit interface.
