//! Transforms applied to the event stream before step results are
//! accumulated.
//!
//! A transform receives the whole event stream and returns a new one. It
//! must preserve the event structure (start/delta/end pairs, step
//! boundaries) so that the event processor can still build step results.

mod smooth;

use crate::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_tool::ToolSet;
use tokio_util::sync::CancellationToken;

use super::result::EventStream;

pub use smooth::ChunkDetector;
pub use smooth::Chunking;
pub use smooth::SmoothStream;
pub use smooth::SmoothStreamConfig;
pub use smooth::smooth_stream;

/// Information handed to a transform when it is applied.
#[derive(Clone)]
pub struct TransformContext {
    tools: Arc<ToolSet>,
    cancellation: CancellationToken,
    stop: CancellationToken,
    failure: Arc<Mutex<Option<Error>>>,
}

impl TransformContext {
    pub(crate) fn new(tools: Arc<ToolSet>, cancellation: CancellationToken) -> Self {
        Self {
            tools,
            cancellation,
            stop: CancellationToken::new(),
            failure: Arc::new(Mutex::new(None)),
        }
    }

    /// Token that fires when a transform calls [`stop`](Self::stop); the
    /// pipeline gates further events on it.
    pub(crate) fn stop_token(&self) -> CancellationToken {
        self.stop.clone()
    }

    pub(crate) fn take_failure(&self) -> Option<Error> {
        self.failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    pub(crate) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Stops the call with an application-supplied transform error.
    ///
    /// The first failure wins. End the transform's output stream after this
    /// call; final-result waiters receive this error rather than cancellation.
    pub fn fail(&self, error: Error) {
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if failure.is_none() {
            *failure = Some(error);
        }
        drop(failure);
        self.stop();
    }

    /// The tools available to the call.
    #[must_use]
    pub fn tools(&self) -> &ToolSet {
        &self.tools
    }

    /// Stops the call: the model stream and pending tool executions are
    /// cancelled and the call ends with [`crate::Error::Cancelled`]. A
    /// transform that stops the call should also end its output stream so
    /// that no further events reach the consumer.
    pub fn stop(&self) {
        self.stop.cancel();
        self.cancellation.cancel();
    }
}

impl fmt::Debug for TransformContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransformContext")
            .field("tools", &self.tools.names().collect::<Vec<_>>())
            .field("stopped", &self.stop.is_cancelled())
            .finish()
    }
}

/// A stream transform.
///
/// Implemented for every `Fn(EventStream, TransformContext) -> EventStream`
/// closure.
pub trait StreamTransform: Send + Sync {
    /// Wraps `input`.
    fn apply(&self, input: EventStream, ctx: TransformContext) -> EventStream;
}

impl<F> StreamTransform for F
where
    F: Fn(EventStream, TransformContext) -> EventStream + Send + Sync,
{
    fn apply(&self, input: EventStream, ctx: TransformContext) -> EventStream {
        self(input, ctx)
    }
}
