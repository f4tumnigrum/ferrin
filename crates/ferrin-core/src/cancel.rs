//! Cancellation plumbing: derived tokens plus the reason they were cancelled.

use std::future::Future;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::timeout::TimeoutScope;

/// Why a call token was cancelled.
#[derive(Debug, Clone)]
pub(crate) enum CancelReason {
    /// A timeout fired.
    Timeout {
        /// Which timeout.
        scope: TimeoutScope,
        /// Elapsed time when it fired.
        elapsed: Duration,
    },
}

/// A cancellation token derived for one call together with the reason cell
/// shared by every token derived from it.
#[derive(Debug, Clone)]
pub(crate) struct CallCancellation {
    token: CancellationToken,
    reason: Arc<OnceLock<CancelReason>>,
}

impl CallCancellation {
    /// Derives a call token from the caller's token.
    pub(crate) fn new(parent: &CancellationToken) -> Self {
        Self {
            token: parent.child_token(),
            reason: Arc::new(OnceLock::new()),
        }
    }

    /// Derives a child token that shares the reason cell.
    pub(crate) fn child(&self) -> Self {
        Self {
            token: self.token.child_token(),
            reason: Arc::clone(&self.reason),
        }
    }

    /// The token.
    pub(crate) fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Returns `true` when the token is cancelled.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// The error that describes the cancellation: a timeout when one was
    /// recorded, otherwise caller cancellation.
    pub(crate) fn error(&self) -> Error {
        match self.reason.get() {
            Some(CancelReason::Timeout { scope, elapsed }) => Error::Timeout {
                scope: scope.clone(),
                elapsed: *elapsed,
            },
            None => Error::Cancelled,
        }
    }

    /// Maps a core error, turning `Cancelled` into the recorded reason.
    pub(crate) fn map_error(&self, error: Error) -> Error {
        match error {
            Error::Cancelled => self.error(),
            other => other,
        }
    }

    /// Cancels the token because `scope` timed out.
    pub(crate) fn cancel_for_timeout(&self, scope: TimeoutScope, elapsed: Duration) {
        let _ = self.reason.set(CancelReason::Timeout { scope, elapsed });
        self.token.cancel();
    }

    /// Runs `future` under an optional timeout for `scope`; on expiry the
    /// token is cancelled and [`Error::Timeout`] is returned.
    pub(crate) async fn with_timeout<T>(
        &self,
        scope: TimeoutScope,
        duration: Option<Duration>,
        future: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        let Some(duration) = duration else {
            return future.await;
        };
        let start = Instant::now();
        match tokio::time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_) => {
                let elapsed = start.elapsed();
                self.cancel_for_timeout(scope.clone(), elapsed);
                Err(Error::Timeout { scope, elapsed })
            }
        }
    }
}
