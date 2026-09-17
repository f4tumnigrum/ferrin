# ferrin-policy

Policy-based tool approval for Ferrin. A `PolicyClient` evaluates a policy
path with a JSON input and returns a decision document; `policy_approval`
turns the client into a `ferrin_core::generate_text::ApprovalPolicy` whose
decisions (`allow`, `deny`, `requires-approval`, `not-applicable`) become
tool approval statuses, and `capability_middleware` restricts the tools
offered to the model through the same client. `HttpPolicyClient` speaks the
OPA REST Data API; `RegoPolicyClient` (feature `rego`) evaluates Rego
policies in-process with `regorus`.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/18-policy-approval.md`, ADR 0020.

## Example

```rust
use ferrin_policy::HttpPolicyClient;
use ferrin_policy::policy_approval;
use ferrin_provider_util::secure_url::UrlPolicy;

fn approval() -> Result<impl ferrin_core::generate_text::ApprovalPolicy, ferrin_policy::PolicyError> {
    // An OPA sidecar on localhost needs the relaxed URL policy; remote
    // policy servers use the default (HTTPS, public networks only).
    let url = url::Url::parse("http://127.0.0.1:8181")
        .map_err(|error| ferrin_policy::PolicyError::InvalidUrl { message: error.to_string() })?;
    let client = HttpPolicyClient::builder(url)
        .url_policy(UrlPolicy::new().allow_http().allow_private_networks())
        .build()?;
    Ok(policy_approval(client, "ferrin/tools/decision"))
}
```

The policy receives `{ "tool": { "name" }, "args", "messages", "runtimeContext" }`
and answers with an explicit decision object or a legacy `{ "allow": bool }`.
Bare booleans are rejected. A `null` result is `NotApplicable`, which overrides
`needs_approval`; use `with_default(policy, ApprovalStatus::user_approval())` to
require approval for unmatched rules. Shadow observe mode approves calls;
backend failures deny unless `FailureMode::FallThrough` is explicitly chosen.

`shadow(policy).on_decision(|event| async move { ... })` observes normalized
decisions, effective decisions, enforcement mode, tool identifiers/input and
the UTC evaluation time. Auditing runs independently; errors and task panics
do not change approval. Keep the policy in an `Arc` and pass a clone to
`.tool_approval(...)`, then call `.flush_decisions().await` before dropping it
when all audit callbacks must finish. Dropping the policy cancels pending
callbacks. The explicit `on_decision_sync` extension retains the synchronous
`(call, status)` observer and blocks approval while that callback runs.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `rego` | off | `RegoPolicyClient`: in-process Rego evaluation with `regorus` |

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
