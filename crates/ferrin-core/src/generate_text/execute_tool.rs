//! Execution of one tool call with timeout, cancellation and telemetry.

use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use ferrin_spec::JsonValue;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolOutput;
use ferrin_tool::ToolOutputStream;
use futures_core::Stream;
use pin_project_lite::pin_project;
use tokio::time::Sleep;

/// One event of a tool execution.
#[derive(Debug)]
pub(crate) enum ToolExecutionEvent {
    /// An intermediate result.
    Preliminary(JsonValue),
    /// The execution finished.
    Finished {
        /// The final output or the error.
        output: Result<JsonValue, ToolError>,
    },
}

pin_project! {
    /// Drives a tool output stream, enforcing a timeout and the context's
    /// cancellation token; ends after the `Finished` event.
    pub(crate) struct ToolExecution {
        #[pin]
        inner: Option<ToolOutputStream>,
        #[pin]
        deadline: Option<Sleep>,
        timeout: Option<Duration>,
        cancellation: tokio_util::sync::CancellationToken,
        #[pin]
        cancelled: tokio_util::sync::WaitForCancellationFutureOwned,
        done: bool,
    }
}

/// Starts executing `tool` with `input`.
///
/// Tools without executor finish immediately with an error; the returned
/// stream yields preliminary results followed by exactly one `Finished`.
pub(crate) fn execute_tool(
    tool: &Arc<Tool>,
    input: JsonValue,
    ctx: ToolContext,
    timeout: Option<Duration>,
) -> ToolExecution {
    let cancellation = ctx.cancellation.clone();
    let inner = tool.execute(input, ctx);
    ToolExecution {
        inner,
        deadline: timeout.map(tokio::time::sleep),
        timeout,
        cancelled: cancellation.clone().cancelled_owned(),
        cancellation,
        done: false,
    }
}

impl Stream for ToolExecution {
    type Item = ToolExecutionEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if *this.done {
            return Poll::Ready(None);
        }
        let finish = |done: &mut bool, output: Result<JsonValue, ToolError>| {
            *done = true;
            Poll::Ready(Some(ToolExecutionEvent::Finished { output }))
        };
        let Some(inner) = this.inner.as_mut().as_pin_mut() else {
            return finish(
                this.done,
                Err(ToolError::message("tool has no execute function")),
            );
        };
        if this.cancelled.as_mut().poll(cx).is_ready() {
            return finish(this.done, Err(ToolError::Cancelled));
        }
        if let Some(deadline) = this.deadline.as_mut().as_pin_mut()
            && deadline.poll(cx).is_ready()
        {
            this.cancellation.cancel();
            return finish(
                this.done,
                Err(ToolError::Timeout(this.timeout.unwrap_or_default())),
            );
        }
        match inner.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(ToolOutput::Preliminary(value)))) => {
                Poll::Ready(Some(ToolExecutionEvent::Preliminary(value)))
            }
            Poll::Ready(Some(Ok(ToolOutput::Final(value)))) => finish(this.done, Ok(value)),
            Poll::Ready(Some(Ok(_))) => finish(
                this.done,
                Err(ToolError::message("unsupported tool output variant")),
            ),
            Poll::Ready(Some(Err(error))) => finish(this.done, Err(error)),
            Poll::Ready(None) => finish(
                this.done,
                Err(ToolError::message("tool produced no final output")),
            ),
        }
    }
}

impl std::fmt::Debug for ToolExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecution")
            .field("timeout", &self.timeout)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}
