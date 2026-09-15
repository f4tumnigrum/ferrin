use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_policy::Enforcement;
use ferrin_policy::shadow;
use pretty_assertions::assert_eq;

use super::common::delete_call;
use super::common::empty_context;

#[tokio::test]
async fn observe_mode_reports_but_does_not_enforce() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let policy =
        shadow(ApprovalStatus::denied().with_reason("shadow")).on_decision(move |call, status| {
            log.lock()
                .unwrap()
                .push((call.tool_name.to_string(), status.cloned()));
        });
    assert_eq!(policy.resolve(&delete_call(), empty_context()).await, None);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(
            "delete_file".to_owned(),
            Some(ApprovalStatus::denied().with_reason("shadow"))
        )]
    );
}

#[tokio::test]
async fn enforce_mode_returns_the_decision() {
    let policy = shadow(ApprovalStatus::user_approval()).enforcement(Enforcement::Enforce);
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::user_approval())
    );
    let debug = format!("{policy:?}");
    assert!(debug.contains("Enforce"), "{debug}");
}
