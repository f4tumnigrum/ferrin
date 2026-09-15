use std::sync::Arc;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::Timeout;
use ferrin_core::generate_text;
use ferrin_core::timeout::TimeoutScope;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

use super::common::mock;
use super::common::text_model;

#[tokio::test]
async fn cancelled_before_start_returns_cancelled_without_calling_the_model() {
    let model = text_model("x");
    let token = CancellationToken::new();
    token.cancel();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .cancellation(token)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(error.is_cancelled());
    assert_eq!(model.call_count(), 0);
}

#[tokio::test(start_paused = true)]
async fn cancelling_during_the_model_call_propagates_to_the_provider() {
    let model = mock()
        .generate_with(|options: CallOptions| async move {
            options.cancellation.cancelled().await;
            Err(ProviderError::Cancelled)
        })
        .build_shared();
    let token = CancellationToken::new();
    let call = generate_text(Arc::clone(&model))
        .prompt("hi")
        .cancellation(token.clone());
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token.cancel();
    };
    let (outcome, ()) = tokio::join!(call, cancel);
    let error = outcome.unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    assert_eq!(model.call_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn total_timeout_fires_while_the_model_hangs() {
    let model = mock()
        .generate_with(|_: CallOptions| std::future::pending())
        .build_shared();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .timeout(Timeout::none().with_total(Duration::from_secs(3)))
        .await
        .unwrap_err();
    match error {
        Error::Timeout { scope, elapsed } => {
            assert_eq!(scope, TimeoutScope::Total);
            assert!(elapsed >= Duration::from_secs(3));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn step_timeout_fires_per_step() {
    let model = mock()
        .generate_with(|_: CallOptions| std::future::pending())
        .build_shared();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .timeout(Timeout::none().with_step(Duration::from_secs(1)))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Timeout {
                scope: TimeoutScope::Step,
                ..
            }
        ),
        "{error:?}"
    );
}

struct ToolDropGuard(Arc<std::sync::atomic::AtomicBool>);
impl Drop for ToolDropGuard {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn cancelling_pending_tools_wakes_and_drops_execution_in_both_loops() {
    for streaming in [false, true] {
        let started = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let notify = Arc::clone(&started);
        let guard = Arc::clone(&dropped);
        let tool = ferrin_tool::Tool::function_with_schema(ferrin_tool::Schema::empty_object())
            .execute(
                move |_: ferrin_spec::JsonValue, _: ferrin_tool::ToolContext| {
                    let notify = Arc::clone(&notify);
                    let guard = ToolDropGuard(Arc::clone(&guard));
                    async move {
                        let _guard = guard;
                        notify.notify_one();
                        std::future::pending::<
                                Result<ferrin_spec::JsonValue, ferrin_tool::ToolError>,
                            >()
                            .await
                    }
                },
            )
            .build();
        let tools = ferrin_tool::ToolSet::new().insert("pending", tool).unwrap();
        let model = mock()
            .generate(super::common::tool_call_result(
                "call",
                "pending",
                &serde_json::json!({}),
            ))
            .stream(vec![
                ferrin_spec::StreamPart::stream_start(),
                ferrin_spec::StreamPart::ToolCall(ferrin_spec::ToolCall::new(
                    "call", "pending", "{}",
                )),
                ferrin_spec::StreamPart::finish(
                    ferrin_spec::FinishReason::tool_calls(),
                    ferrin_spec::Usage::default(),
                ),
            ])
            .build_shared();
        let token = CancellationToken::new();
        let call = async {
            if streaming {
                ferrin_core::stream_text(model)
                    .prompt("hi")
                    .tools(tools)
                    .cancellation(token.clone())
                    .await
                    .unwrap()
                    .consume()
                    .await
            } else {
                generate_text(model)
                    .prompt("hi")
                    .tools(tools)
                    .cancellation(token.clone())
                    .await
            }
        };
        let cancel = async {
            started.notified().await;
            token.cancel();
        };
        let (outcome, ()) =
            tokio::join!(tokio::time::timeout(Duration::from_secs(1), call), cancel);
        let error = outcome
            .expect("cancellation must wake the pending tool")
            .unwrap_err();
        assert!(error.is_cancelled(), "{error:?}");
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }
}
