use ferrin_core::generate_text::ApprovalStatus;
use ferrin_policy::PolicyDecision;
use ferrin_policy::UNRECOGNIZED_DECISION;
use pretty_assertions::assert_eq;
use serde_json::json;

fn unrecognized() -> PolicyDecision {
    PolicyDecision::deny().with_reason(UNRECOGNIZED_DECISION)
}

#[test]
fn normalizes_decision_documents() {
    let cases = [
        (json!(null), PolicyDecision::NotApplicable),
        (json!(true), PolicyDecision::allow()),
        (json!(false), PolicyDecision::deny()),
        (json!({ "decision": "allow" }), PolicyDecision::allow()),
        (
            json!({ "decision": "deny", "reason": "no" }),
            PolicyDecision::deny().with_reason("no"),
        ),
        (
            json!({ "decision": "requires-approval", "reason": "ask" }),
            PolicyDecision::requires_approval().with_reason("ask"),
        ),
        (
            json!({ "decision": "not-applicable", "reason": "ignored" }),
            PolicyDecision::NotApplicable,
        ),
        (json!({ "allow": true }), PolicyDecision::allow()),
        (
            json!({ "allow": false, "reason": "legacy" }),
            PolicyDecision::deny().with_reason("legacy"),
        ),
        (json!({ "decision": "maybe" }), unrecognized()),
        (json!({ "decision": 1 }), unrecognized()),
        (json!({ "allow": "yes" }), unrecognized()),
        (json!({}), unrecognized()),
        (json!("allow"), unrecognized()),
        (json!(["allow"]), unrecognized()),
        (json!(1), unrecognized()),
    ];
    for (raw, expected) in cases {
        assert_eq!(PolicyDecision::normalize(&raw), expected, "{raw}");
    }
}

#[test]
fn maps_to_approval_statuses() {
    assert_eq!(
        PolicyDecision::allow().with_reason("ok").into_approval(),
        Some(ApprovalStatus::approved().with_reason("ok"))
    );
    assert_eq!(
        PolicyDecision::deny().into_approval(),
        Some(ApprovalStatus::denied())
    );
    assert_eq!(
        PolicyDecision::requires_approval().into_approval(),
        Some(ApprovalStatus::user_approval())
    );
    assert_eq!(PolicyDecision::NotApplicable.into_approval(), None);
    assert_eq!(
        PolicyDecision::NotApplicable.with_reason("x").reason(),
        None
    );
}

#[test]
fn serializes_like_the_wire_format() {
    let decision = PolicyDecision::requires_approval().with_reason("ask");
    let json = serde_json::to_value(&decision).unwrap();
    assert_eq!(
        json,
        json!({ "decision": "requires-approval", "reason": "ask" })
    );
    assert_eq!(
        serde_json::from_value::<PolicyDecision>(json).unwrap(),
        decision
    );
    assert_eq!(
        serde_json::to_value(PolicyDecision::NotApplicable).unwrap(),
        json!({ "decision": "not-applicable" })
    );
}
