use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_policy::PolicyClient;
use ferrin_policy::PolicyError;
use ferrin_policy::RegoPolicyClient;
use ferrin_policy::policy_approval;
use ferrin_spec::JsonValue;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::delete_call;
use super::common::empty_context;

const POLICY: &str = r#"
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

allow if input.tool.name in data.allowed_tools

allowed := [name | some tool in input.tools; name := tool.name; name in data.allowed_tools]

undefined_unless_admin := true if input.user == "admin"
"#;

fn client() -> RegoPolicyClient {
    RegoPolicyClient::builder()
        .policy("ferrin.rego", POLICY)
        .data(json!({ "allowed_tools": ["read_file", "search"] }))
        .build()
        .unwrap()
}

#[tokio::test]
async fn evaluates_rules_with_input_and_data() {
    let client = client();
    assert_eq!(client.packages(), &["data.ferrin.tools".to_owned()]);

    let deny = client
        .evaluate(
            "ferrin/tools/decision",
            json!({ "tool": { "name": "delete_file" }, "input": { "path": "/tmp/x" } }),
        )
        .await
        .unwrap();
    assert_eq!(
        deny,
        json!({ "decision": "deny", "reason": "protected path" })
    );

    let ask = client
        .evaluate(
            "ferrin.tools.decision",
            json!({ "tool": { "name": "delete_file" }, "input": { "path": "/home/x" } }),
        )
        .await
        .unwrap();
    assert_eq!(ask, json!({ "decision": "requires-approval" }));

    let default = client
        .evaluate(
            "data.ferrin.tools.decision",
            json!({ "tool": { "name": "other" } }),
        )
        .await
        .unwrap();
    assert_eq!(default, json!({ "decision": "not-applicable" }));

    let allow = client
        .evaluate(
            "ferrin/tools/allow",
            json!({ "tool": { "name": "search" } }),
        )
        .await
        .unwrap();
    assert_eq!(allow, json!(true));

    let allowed = client
        .evaluate(
            "ferrin/tools/allowed",
            json!({ "tools": [{ "name": "search" }, { "name": "delete_file" }] }),
        )
        .await
        .unwrap();
    assert_eq!(allowed, json!(["search"]));
}

#[tokio::test]
async fn undefined_values_are_null_and_unknown_rules_are_errors() {
    let client = client();
    let undefined = client
        .evaluate(
            "ferrin/tools/allow",
            json!({ "tool": { "name": "delete_file" } }),
        )
        .await
        .unwrap();
    assert_eq!(undefined, JsonValue::Null);
    let undefined = client
        .evaluate(
            "ferrin/tools/undefined_unless_admin",
            json!({ "user": "guest" }),
        )
        .await
        .unwrap();
    assert_eq!(undefined, JsonValue::Null);

    assert!(matches!(
        client
            .evaluate("ferrin/tools/missing", json!({}))
            .await
            .unwrap_err(),
        PolicyError::Engine { .. }
    ));
}

#[test]
fn invalid_policies_fail_to_build() {
    let error = RegoPolicyClient::from_policy("bad.rego", "package x\n allow if {").unwrap_err();
    assert!(matches!(error, PolicyError::Engine { .. }), "{error:?}");
}

#[tokio::test]
async fn drives_tool_approval() {
    let policy = policy_approval(client(), "ferrin/tools/decision");
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::denied().with_reason("protected path"))
    );
    let missing = policy_approval(client(), "ferrin/tools/missing");
    let status = missing.resolve(&delete_call(), empty_context()).await;
    assert!(
        matches!(&status, Some(ApprovalStatus::Denied { reason: Some(reason) }) if reason == "policy evaluation failed"),
        "{status:?}"
    );
}
