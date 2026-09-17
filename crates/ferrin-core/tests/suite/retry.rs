use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::RetryPolicy;
use ferrin_core::error::RetryReason;
use ferrin_core::generate_text;
use ferrin_core::stream_text;
use ferrin_testing::api_call_error;
use futures_util::FutureExt;
use http::StatusCode;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

use super::common::mock;
use super::common::text_result;

#[tokio::test(start_paused = true)]
async fn retryable_errors_are_retried_with_backoff() {
    let model = mock()
        .generate_error(api_call_error(StatusCode::INTERNAL_SERVER_ERROR, "boom"))
        .generate_error(api_call_error(StatusCode::TOO_MANY_REQUESTS, "slow"))
        .generate(text_result("ok"))
        .build_shared();
    let start = tokio::time::Instant::now();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .retry_policy(RetryPolicy::default())
        .await
        .unwrap();
    assert_eq!(result.text(), "ok");
    assert_eq!(model.call_count(), 3);
    // 2 s + 4 s of exponential backoff.
    assert_eq!(start.elapsed().as_secs(), 6);
}

#[tokio::test(start_paused = true)]
async fn exhausted_retries_report_every_attempt() {
    let model = mock()
        .generate_repeat(text_result("never"))
        .generate_error(api_call_error(StatusCode::BAD_GATEWAY, "1"))
        .generate_error(api_call_error(StatusCode::BAD_GATEWAY, "2"))
        .generate_error(api_call_error(StatusCode::BAD_GATEWAY, "3"))
        .build_shared();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .await
        .unwrap_err();
    match error {
        Error::Retry {
            reason,
            attempts,
            errors,
        } => {
            assert_eq!(reason, RetryReason::MaxRetriesExceeded);
            assert_eq!(attempts, 3);
            assert_eq!(errors.len(), 3);
        }
        other => panic!("unexpected error {other:?}"),
    }
    assert_eq!(model.call_count(), 3);
}

#[tokio::test]
async fn non_retryable_first_failure_is_returned_directly() {
    let model = mock()
        .generate_error(api_call_error(StatusCode::BAD_REQUEST, "bad"))
        .build_shared();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Provider(_)), "{error:?}");
    assert_eq!(error.status_code(), Some(StatusCode::BAD_REQUEST));
    assert!(!error.is_retryable());
    assert_eq!(model.call_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn retry_policy_none_disables_retries() {
    let model = mock()
        .generate_error(api_call_error(StatusCode::INTERNAL_SERVER_ERROR, "boom"))
        .build_shared();
    let error = generate_text(Arc::clone(&model))
        .prompt("hi")
        .retry_policy(RetryPolicy::none())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Provider(_)), "{error:?}");
    assert!(error.is_retryable());
    assert_eq!(model.call_count(), 1);
}

#[tokio::test]
async fn invalid_backoff_factors_fail_before_provider_invocation() {
    for backoff_factor in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let model = mock().generate_repeat(text_result("unused")).build_shared();
        let policy = RetryPolicy {
            backoff_factor,
            ..RetryPolicy::default()
        };
        let generated = generate_text(Arc::clone(&model))
            .prompt("hi")
            .retry_policy(policy.clone())
            .await;
        let streamed = stream_text(Arc::clone(&model))
            .prompt("hi")
            .retry_policy(policy)
            .await;
        assert!(matches!(
            generated,
            Err(Error::InvalidArgument { argument, .. }) if argument == "retry_policy.backoff_factor"
        ));
        assert!(matches!(
            streamed,
            Err(Error::InvalidArgument { argument, .. }) if argument == "retry_policy.backoff_factor"
        ));
        assert_eq!(model.call_count(), 0);
    }
}

#[test]
fn delay_arithmetic_saturates_and_preserves_zero_initial_delay() {
    let error = api_call_error(StatusCode::TOO_MANY_REQUESTS, "slow");
    let policy = RetryPolicy {
        initial_delay: Duration::from_secs(1),
        backoff_factor: f64::MAX,
        ..RetryPolicy::default()
    };
    assert_eq!(policy.delay_for(u32::MAX, &error), Duration::MAX);
    let zero = RetryPolicy {
        initial_delay: Duration::ZERO,
        ..policy
    };
    assert_eq!(zero.delay_for(u32::MAX, &error), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn oversized_retry_delays_remain_cancellable() {
    let model = mock()
        .generate_error(api_call_error(StatusCode::TOO_MANY_REQUESTS, "slow"))
        .build_shared();
    let token = CancellationToken::new();
    let mut call = generate_text(Arc::clone(&model))
        .prompt("hi")
        .cancellation(token.clone())
        .retry_policy(RetryPolicy {
            initial_delay: Duration::MAX,
            ..RetryPolicy::default()
        })
        .into_future();
    assert!(call.as_mut().now_or_never().is_none());
    assert_eq!(model.call_count(), 1);
    token.cancel();
    let error = call.await.unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    assert_eq!(model.call_count(), 1);
}
