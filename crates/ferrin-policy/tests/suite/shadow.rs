use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use chrono::Utc;
use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_policy::Enforcement;
use ferrin_policy::PolicyDecisionEvent;
use ferrin_policy::PolicyDecisionToolCall;
use ferrin_policy::shadow;
use ferrin_spec::ToolName;
use pretty_assertions::assert_eq;
use tokio::sync::Notify;

use super::common::delete_call;
use super::common::empty_context;

#[tokio::test]
async fn observe_and_enforce_report_the_normalized_and_effective_decisions() {
    for enforcement in [Enforcement::Observe, Enforcement::Enforce] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let decision = ApprovalStatus::denied().with_reason("shadow");
        let policy = shadow(decision.clone())
            .enforcement(enforcement)
            .on_decision(move |event| {
                let log = Arc::clone(&log);
                async move { log.lock().unwrap().push(event) }
            });
        let effective = if enforcement == Enforcement::Enforce {
            decision.clone()
        } else {
            ApprovalStatus::approved()
        };
        let before = Utc::now();
        assert_eq!(
            policy.resolve(&delete_call(), empty_context()).await,
            Some(effective.clone())
        );
        policy.flush_decisions().await;
        let events = seen.lock().unwrap().clone();
        let timestamp = events[0].timestamp;
        let call = delete_call();
        assert_eq!(
            (events, (before..=Utc::now()).contains(&timestamp)),
            (
                vec![PolicyDecisionEvent {
                    tool_call: PolicyDecisionToolCall {
                        tool_name: call.tool_name,
                        tool_call_id: call.tool_call_id,
                        input: call.input,
                    },
                    decision,
                    enforced: enforcement == Enforcement::Enforce,
                    effective,
                    timestamp,
                }],
                true,
            )
        );
    }
}

#[tokio::test]
async fn absent_status_is_normalized_in_the_event_and_enforcement() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let policy = shadow(HashMap::<ToolName, ApprovalStatus>::new())
        .enforcement(Enforcement::Enforce)
        .on_decision(move |event| {
            let log = Arc::clone(&log);
            async move { log.lock().unwrap().push((event.decision, event.effective)) }
        });
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::NotApplicable)
    );
    policy.flush_decisions().await;
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(ApprovalStatus::NotApplicable, ApprovalStatus::NotApplicable)]
    );
}

#[tokio::test]
async fn observer_errors_and_panics_never_change_approval() {
    let policy = shadow(ApprovalStatus::denied())
        .enforcement(Enforcement::Enforce)
        .on_decision(|_| async { Err::<(), _>("audit unavailable") });
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::denied())
    );
    policy.flush_decisions().await;

    let policy = shadow(ApprovalStatus::denied()).on_decision(|_| async {
        tokio::task::yield_now().await;
        panic!("async observer panic");
    });
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::approved())
    );
    policy.flush_decisions().await;

    let policy = shadow(ApprovalStatus::denied()).on_decision(|_| -> std::future::Ready<()> {
        panic!("observer construction panic");
    });
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::approved())
    );
    policy.flush_decisions().await;
}

#[tokio::test]
async fn pending_audit_and_concurrent_flush_do_not_block_new_decisions() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let completed = Arc::new(Notify::new());
    let policy = shadow(ApprovalStatus::denied()).on_decision({
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        let completed = Arc::clone(&completed);
        move |event| {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let completed = Arc::clone(&completed);
            async move {
                if event.tool_call.tool_call_id.as_str() == "first" {
                    started.notify_one();
                    release.notified().await;
                } else {
                    completed.notify_one();
                }
            }
        }
    });
    let mut first = delete_call();
    first.tool_call_id = "first".into();
    assert_eq!(
        policy.resolve(&first, empty_context()).await,
        Some(ApprovalStatus::approved())
    );
    started.notified().await;
    let flush = policy.flush_decisions();
    tokio::pin!(flush);
    let second = delete_call();
    tokio::select! {
        biased;
        () = &mut flush => panic!("first callback is still blocked"),
        result = policy.resolve(&second, empty_context()) => {
            assert_eq!(result, Some(ApprovalStatus::approved()));
        }
    }
    completed.notified().await;
    release.notify_one();
    flush.await;
    policy.flush_decisions().await;
}

struct Dropped(Arc<Notify>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test]
async fn dropping_the_policy_cancels_its_pending_audit_tasks() {
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let policy = shadow(ApprovalStatus::approved()).on_decision({
        let started = Arc::clone(&started);
        let dropped = Arc::clone(&dropped);
        move |_| {
            let started = Arc::clone(&started);
            let guard = Dropped(Arc::clone(&dropped));
            async move {
                let _guard = guard;
                started.notify_one();
                std::future::pending::<()>().await;
            }
        }
    });
    policy.resolve(&delete_call(), empty_context()).await;
    started.notified().await;
    drop(policy);
    dropped.notified().await;
}

#[tokio::test]
async fn synchronous_extension_observes_raw_status_and_isolates_panics() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let policy =
        shadow(HashMap::<ToolName, ApprovalStatus>::new()).on_decision_sync(move |call, status| {
            log.lock()
                .unwrap()
                .push((call.tool_name.clone(), status.cloned()));
            panic!("sync audit failure");
        });
    assert_eq!(
        policy.resolve(&delete_call(), empty_context()).await,
        Some(ApprovalStatus::approved())
    );
    assert_eq!(*seen.lock().unwrap(), vec![("delete_file".into(), None)]);
}
