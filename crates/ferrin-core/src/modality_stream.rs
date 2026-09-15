//! Total deadlines and cancellation for streaming audio modalities.

use std::future::Future;
use std::time::Duration;

use ferrin_spec::BoxStream;
use ferrin_spec::language_model::StreamError;
use futures_util::StreamExt;
use futures_util::stream;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::timeout::TimeoutScope;

pub(crate) struct StreamDeadline {
    pub(crate) cancellation: CancellationToken,
    started: Instant,
    deadline: Option<Instant>,
}

impl StreamDeadline {
    pub(crate) fn new(parent: &CancellationToken, timeout: Option<Duration>) -> Self {
        let started = Instant::now();
        Self {
            cancellation: parent.child_token(),
            started,
            deadline: timeout.and_then(|duration| started.checked_add(duration)),
        }
    }

    pub(crate) async fn run<T>(
        &self,
        future: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        let expires = async {
            match self.deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(Error::Cancelled),
            () = expires => {
                self.cancellation.cancel();
                Err(Error::Timeout { scope: TimeoutScope::Total, elapsed: self.started.elapsed() })
            }
            result = future => result,
        }
    }

    pub(crate) fn wrap<T: Send + 'static>(
        self,
        parts: BoxStream<'static, T>,
        error_part: fn(StreamError) -> T,
        is_terminal: fn(&T) -> bool,
    ) -> BoxStream<'static, T> {
        Box::pin(stream::unfold(
            Some((self, parts)),
            move |state| async move {
                let (deadline, mut parts) = state?;
                match deadline.run(async { Ok(parts.next().await) }).await {
                    Ok(Some(part)) => {
                        let next = if is_terminal(&part) {
                            None
                        } else {
                            Some((deadline, parts))
                        };
                        Some((part, next))
                    }
                    Ok(None) => None,
                    Err(error) => {
                        let error_type = if matches!(error, Error::Timeout { .. }) {
                            "timeout"
                        } else {
                            "cancelled"
                        };
                        let mut error = StreamError::new(error.to_string());
                        error.error_type = Some(error_type.to_owned());
                        error.is_retryable = Some(false);
                        Some((error_part(error), None))
                    }
                }
            },
        ))
    }
}

impl Drop for StreamDeadline {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
