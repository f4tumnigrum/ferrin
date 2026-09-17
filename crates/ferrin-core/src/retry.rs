//! Retry policy and the retry loop used around provider calls.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::future::Future;
use std::time::Duration;

use ferrin_provider_util::retry::retry_after_within;
use ferrin_spec::error::ProviderError;
use futures_util::future::Either;
use futures_util::future::select;
use rand::RngExt;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::error::RetryReason;

/// How failed provider calls are retried.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Maximum number of retries after the first attempt (default 2).
    pub max_retries: u32,
    /// Delay before the first retry (default 2 s).
    pub initial_delay: Duration,
    /// Multiplier applied to the delay after each retry (default 2.0).
    pub backoff_factor: f64,
    /// Largest `Retry-After` value that is honoured (default 60 s).
    pub max_retry_after: Duration,
    /// Random jitter applied to the computed delay (default none).
    pub jitter: Jitter,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_secs(2),
            backoff_factor: 2.0,
            max_retry_after: Duration::from_secs(60),
            jitter: Jitter::None,
        }
    }
}

impl RetryPolicy {
    /// Creates the default policy with a different retry count.
    #[must_use]
    pub fn with_max_retries(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Self::default()
        }
    }

    /// Disables retries.
    #[must_use]
    pub fn none() -> Self {
        Self::with_max_retries(0)
    }

    /// Computes the delay before retry number `retry` (1-based), honouring
    /// `Retry-After` headers when they fall within `max_retry_after`.
    ///
    /// Arithmetic overflow saturates at [`Duration::MAX`]. Provider calls
    /// reject negative or non-finite backoff factors before using this policy.
    #[must_use]
    pub fn delay_for(&self, retry: u32, error: &ProviderError) -> Duration {
        let header_delay = error
            .as_api_call()
            .and_then(|api| api.response_headers.as_ref())
            .and_then(|headers| retry_after_within(headers, self.max_retry_after));
        let base = header_delay.unwrap_or_else(|| {
            let exponent = retry.saturating_sub(1);
            let factor = self
                .backoff_factor
                .powi(i32::try_from(exponent).unwrap_or(i32::MAX));
            if self.initial_delay.is_zero() {
                Duration::ZERO
            } else {
                Duration::try_from_secs_f64(self.initial_delay.as_secs_f64() * factor.max(0.0))
                    .unwrap_or(Duration::MAX)
            }
        });
        match self.jitter {
            Jitter::None => base,
            Jitter::Full => {
                let millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
                if millis == 0 {
                    base
                } else {
                    Duration::from_millis(rand::rng().random_range(0..=millis))
                }
            }
        }
    }
}

/// Jitter applied to retry delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Jitter {
    /// No jitter: deterministic delays.
    #[default]
    None,
    /// Uniformly random delay between zero and the computed delay.
    Full,
}

/// Runs `operation` with retries.
///
/// `operation` receives the attempt number (0 for the first try). Only
/// [`Error::Provider`] failures are retried: a non-retryable error on the
/// first try is returned unwrapped, later failures are reported as
/// [`Error::Retry`], cancellation stops immediately and every other error is
/// returned as is.
pub(crate) async fn retry<T, F, Fut>(
    policy: &RetryPolicy,
    cancellation: &CancellationToken,
    operation: F,
) -> Result<T, Error>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    retry_with(policy, cancellation, ProviderError::is_retryable, operation).await
}

/// Like [`retry`], with a custom decision of which provider errors are
/// retried.
pub(crate) async fn retry_with<T, F, Fut>(
    policy: &RetryPolicy,
    cancellation: &CancellationToken,
    is_retryable: impl Fn(&ProviderError) -> bool,
    mut operation: F,
) -> Result<T, Error>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    if !policy.backoff_factor.is_finite() || policy.backoff_factor < 0.0 {
        return Err(Error::invalid_argument(
            "retry_policy.backoff_factor",
            "must be finite and nonnegative",
        ));
    }
    let mut errors: Vec<ProviderError> = Vec::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(abort_error(errors));
        }
        let attempt = u32::try_from(errors.len()).unwrap_or(u32::MAX);
        match operation(attempt).await {
            Ok(value) => return Ok(value),
            Err(Error::Cancelled) => return Err(abort_error(errors)),
            Err(Error::Provider(error)) => {
                let error = *error;
                if policy.max_retries == 0 {
                    return Err(Error::from(error));
                }
                let retry_number = attempt.saturating_add(1);
                let retryable = retry_number <= policy.max_retries && is_retryable(&error);
                let delay = retryable.then(|| policy.delay_for(retry_number, &error));
                errors.push(error);
                let attempts = retry_number;
                if attempts > policy.max_retries {
                    return Err(Error::Retry {
                        reason: RetryReason::MaxRetriesExceeded,
                        attempts,
                        errors,
                    });
                }
                if !retryable {
                    if attempts == 1 {
                        return Err(Error::from(
                            errors.pop().unwrap_or(ProviderError::Cancelled),
                        ));
                    }
                    return Err(Error::Retry {
                        reason: RetryReason::ErrorNotRetryable,
                        attempts,
                        errors,
                    });
                }
                let sleep = Box::pin(async {
                    match tokio::time::Instant::now().checked_add(delay.unwrap_or_default()) {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                });
                let cancelled = Box::pin(cancellation.cancelled());
                if let Either::Right(_) = select(sleep, cancelled).await {
                    return Err(Error::Retry {
                        reason: RetryReason::Abort,
                        attempts,
                        errors,
                    });
                }
            }
            Err(other) => return Err(other),
        }
    }
}

fn abort_error(errors: Vec<ProviderError>) -> Error {
    if errors.is_empty() {
        Error::Cancelled
    } else {
        Error::Retry {
            reason: RetryReason::Abort,
            attempts: u32::try_from(errors.len()).unwrap_or(u32::MAX),
            errors,
        }
    }
}
