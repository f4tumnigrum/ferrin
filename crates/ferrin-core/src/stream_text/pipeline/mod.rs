//! The streaming pipeline.
//!
//! A producer task ([`stage`]) runs the step loop and emits events through a
//! bounded channel. On the consumer side the user transforms are applied and
//! the event processor ([`processor`]) accumulates step results, runs the
//! hooks and resolves the completion once the stream has been drained.

mod attempt;
mod parts;
mod processor;
mod stage;

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_stream::wrappers::ReceiverStream;
use tracing::Instrument;

use super::StreamEvent;
use super::builder::StreamConfig;
use super::result::Completion;
use super::result::EventStream;
use super::result::StreamTextResult;
use super::transforms::TransformContext;
use crate::error::Error;
use crate::generate_text::StepContent;
use crate::generate_text::config::CallConfig;
use crate::generate_text::run::LoopContext;
use crate::limits::EVENT_CHANNEL_CAPACITY;
use crate::output::OutputHandler;
use crate::telemetry::spans;

/// Converts tool-related step content into its stream event.
pub(super) fn content_event(content: StepContent) -> Option<StreamEvent> {
    match content {
        StepContent::ToolResult(result) => Some(StreamEvent::ToolResult(result)),
        StepContent::ToolError(error) => Some(StreamEvent::ToolError(error)),
        StepContent::ToolOutputDenied(denied) => Some(StreamEvent::ToolOutputDenied(denied)),
        StepContent::ToolApprovalRequest(request) => {
            Some(StreamEvent::ToolApprovalRequest(request))
        }
        StepContent::ToolApprovalResponse(response) => {
            Some(StreamEvent::ToolApprovalResponse(response))
        }
        _ => None,
    }
}

/// Starts the pipeline and resolves once the first model request has been
/// established.
pub(crate) async fn start<O: Send + 'static>(
    config: CallConfig,
    output: Arc<dyn OutputHandler<O>>,
    stream: StreamConfig,
) -> Result<StreamTextResult<O>, Error> {
    let ctx = Arc::new(LoopContext::new(config, output.response_format())?);
    let stream = Arc::new(stream);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
    let (ready_tx, ready_rx) = oneshot::channel();
    let (step_tx, step_rx) = mpsc::channel(1);
    let (outcome_tx, outcome_rx) = oneshot::channel();
    let (completion_tx, completion_rx) = oneshot::channel();

    let mut tasks = JoinSet::new();
    let span = spans::call_span("stream_text", ctx.function_id(), &ctx.identity);
    tasks.spawn(
        stage::run(
            Arc::clone(&ctx),
            Arc::clone(&stream),
            stage::StageChannels {
                events: event_tx,
                ready: ready_tx,
                steps: step_rx,
                outcome: outcome_tx,
            },
        )
        .instrument(span),
    );

    match ready_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            let message = match tasks.join_next().await {
                Some(Err(join_error)) => format!("stream pipeline task failed: {join_error}"),
                _ => "stream pipeline task ended before the first model call".to_owned(),
            };
            return Err(Error::message(message));
        }
    }

    let transform_ctx = TransformContext::new(
        Arc::clone(&ctx.execution_tools),
        ctx.cancellation.token().clone(),
    );
    // Gate: once a transform calls `stop()`, no further event enters the
    // transforms and the stream ends (without `finish`).
    let stop = transform_ctx.stop_token();
    let mut events: EventStream = Box::pin(
        ReceiverStream::new(event_rx).take_while(move |_| std::future::ready(!stop.is_cancelled())),
    );
    for transform in &stream.transforms {
        events = transform.apply(events, transform_ctx.clone());
    }
    let call_id = ctx.call_id.clone();
    let processor = processor::Processor::new(
        Arc::clone(&ctx),
        Arc::clone(&output),
        tasks,
        step_tx,
        outcome_rx,
        completion_tx,
    );
    Ok(StreamTextResult {
        call_id,
        events: Box::pin(processor::process(events, processor)),
        completion: Completion::new(completion_rx),
        output,
    })
}
