//! The producer stage: runs the step loop in a background task and emits
//! events through the bounded channel.

use crate::generate_text::tools::ToolEnvironment;
use std::collections::HashSet;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::Instrument;

use super::attempt::Attempt;
use super::content_event;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::generate_text::ParsedToolCall;
use crate::generate_text::StepContent;
use crate::generate_text::StepResponse;
use crate::generate_text::StepResult;
use crate::generate_text::inputs::StepState;
use crate::generate_text::inputs::emit_model_call_start;
use crate::generate_text::inputs::prepare_step_inputs;
use crate::generate_text::is_stop_condition_met;
use crate::generate_text::replay::replay_approvals;
use crate::generate_text::replay::replay_tool_message;
use crate::generate_text::run::LoopContext;
use crate::generate_text::run::LoopState;
use crate::generate_text::run::is_tool_execution_allowed;
use crate::generate_text::tools::run_tool_call;
use crate::generate_text::tools::track_deferred;
use crate::limits::TOOL_RESULT_CHANNEL_CAPACITY;
use crate::stream_text::StreamErrorInfo;
use crate::stream_text::StreamEvent;
use crate::stream_text::builder::StreamConfig;
use crate::telemetry::ErrorEvent;
use crate::telemetry::ErrorPhase;
use crate::telemetry::spans;
use crate::timeout::TimeoutScope;

/// Channels connecting the producer with the consumer side.
pub(super) struct StageChannels {
    pub(super) events: mpsc::Sender<StreamEvent>,
    pub(super) ready: oneshot::Sender<Result<(), Error>>,
    pub(super) steps: mpsc::Receiver<StepResult>,
    pub(super) outcome: oneshot::Sender<StageOutcome>,
}

/// How the producer ended.
#[derive(Debug)]
pub(super) enum StageOutcome {
    /// The loop finished and the `Finish` event was emitted.
    Finished,
    /// The loop ended with an error (already emitted as `Error` or `Abort`).
    Failed(Error),
}

/// Sends events to the consumer; buffers them until the first model request
/// succeeded so that startup never blocks on channel capacity.
pub(super) struct Emitter {
    tx: mpsc::Sender<StreamEvent>,
    buffer: Option<Vec<StreamEvent>>,
}

impl Emitter {
    /// Sends `event`; a closed channel (consumer dropped) is reported as
    /// cancellation.
    pub(super) async fn send(&mut self, event: StreamEvent) -> Result<(), Error> {
        if let Some(buffer) = &mut self.buffer {
            buffer.push(event);
            return Ok(());
        }
        self.tx.send(event).await.map_err(|_| Error::Cancelled)
    }

    async fn release(&mut self) -> Result<(), Error> {
        if let Some(buffer) = self.buffer.take() {
            for event in buffer {
                self.tx.send(event).await.map_err(|_| Error::Cancelled)?;
            }
        }
        Ok(())
    }
}

struct Stage {
    step_state: StepState,
    ctx: Arc<LoopContext>,
    stream: Arc<StreamConfig>,
    emitter: Emitter,
    ready: Option<oneshot::Sender<Result<(), Error>>>,
    steps_rx: mpsc::Receiver<StepResult>,
    steps: Vec<StepResult>,
    response_messages: Vec<Message>,
    replay_message: Option<Message>,
    pending_deferred: HashSet<ToolCallId>,
    used_part_ids: HashSet<PartId>,
    total_usage: Usage,
    finish_reason: Option<FinishReason>,
    error_reported: bool,
}

/// Runs the producer to completion and reports the outcome.
pub(super) async fn run(ctx: Arc<LoopContext>, stream: Arc<StreamConfig>, channels: StageChannels) {
    let StageChannels {
        events,
        ready,
        steps,
        outcome,
    } = channels;
    let mut stage = Stage {
        step_state: StepState::new(&ctx),
        ctx,
        stream,
        emitter: Emitter {
            tx: events,
            buffer: Some(Vec::new()),
        },
        ready: Some(ready),
        steps_rx: steps,
        steps: Vec::new(),
        response_messages: Vec::new(),
        replay_message: None,
        pending_deferred: HashSet::new(),
        used_part_ids: HashSet::new(),
        total_usage: Usage::default(),
        finish_reason: None,
        error_reported: false,
    };
    let cancellation = stage.ctx.cancellation.child();
    let total = stage.ctx.config.timeout.total;
    let result = Box::pin(cancellation.with_timeout(TimeoutScope::Total, total, stage.run_loop()))
        .await
        .map_err(|error| cancellation.map_error(error));
    let stage_outcome = match result {
        Ok(()) => StageOutcome::Finished,
        Err(error) => {
            if let Some(ready) = stage.ready.take() {
                // The first model request never succeeded: the caller gets the
                // error from `.await` and no result exists.
                let _ = ready.send(Err(error));
                return;
            }
            stage.report_failure(error).await
        }
    };
    let _ = outcome.send(stage_outcome);
}

impl Stage {
    async fn run_loop(&mut self) -> Result<(), Error> {
        self.emitter
            .send(StreamEvent::Start {
                call_id: self.ctx.call_id.clone(),
            })
            .await?;
        self.ctx.emit_start().await;

        let replay = replay_approvals(
            &self.ctx,
            &self.ctx.initial_messages,
            &self.ctx.cancellation,
        )
        .await?;
        for content in &replay {
            if let Some(event) = content_event(content.clone()) {
                self.emitter.send(event).await?;
            }
        }
        self.replay_message = replay_tool_message(&replay, &self.ctx.execution_tools);
        self.response_messages = self.replay_message.iter().cloned().collect();
        let stop_conditions = self.ctx.stop_conditions();

        loop {
            let step_cancellation = self.ctx.cancellation.child();
            let step_timeout = self.ctx.config.timeout.step;
            let step_number = u32::try_from(self.steps.len()).unwrap_or(u32::MAX);
            let state = Box::pin(
                step_cancellation.with_timeout(
                    TimeoutScope::Step,
                    step_timeout,
                    self.run_step(&step_cancellation)
                        .instrument(spans::step_span(step_number)),
                ),
            )
            .await
            .map_err(|error| step_cancellation.map_error(error))?;
            // Wait until the consumer side has built the step result.
            let Some(step) = self.steps_rx.recv().await else {
                return Err(Error::Cancelled);
            };
            let replayed = usize::from(self.steps.is_empty() && self.replay_message.is_some());
            self.response_messages
                .extend(step.response.messages.iter().skip(replayed).cloned());
            self.total_usage = self.total_usage.add(&step.usage);
            self.finish_reason = Some(step.finish_reason.clone());
            self.steps.push(step);
            if !state.should_continue()
                || is_stop_condition_met(&stop_conditions, &self.steps).await
            {
                break;
            }
        }

        let Some(finish_reason) = self.finish_reason.clone() else {
            return Err(Error::NoOutputGenerated);
        };
        self.emitter
            .send(StreamEvent::Finish {
                finish_reason,
                total_usage: self.total_usage.clone(),
            })
            .await
    }

    async fn run_step(&mut self, cancellation: &CallCancellation) -> Result<LoopState, Error> {
        let step_started = Instant::now();
        self.error_reported = false;
        let inputs = prepare_step_inputs(
            &self.ctx,
            &self.steps,
            &self.response_messages,
            &mut self.step_state,
            cancellation,
        )
        .await?;
        emit_model_call_start(&self.ctx, &inputs).await;
        let step_number = inputs.step_number;
        let identity = inputs.identity.clone();
        let mut attempt = Attempt::new(
            Arc::clone(&self.ctx),
            Arc::clone(&self.stream),
            inputs,
            cancellation.clone(),
        );

        let span = spans::model_call_span(&identity);
        let stream = attempt.call_model().instrument(span.clone()).await?;
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(Ok(()));
            self.emitter.release().await?;
        }

        let mut end = match attempt
            .read(&mut self.emitter, stream, span, &mut self.used_part_ids)
            .await
        {
            Ok(end) => end,
            Err(error) => {
                self.error_reported = attempt.error_reported;
                return Err(error);
            }
        };

        let client_tool_calls = end
            .tool_calls
            .iter()
            .filter(|call| !call.provider_executed)
            .count();
        let mut client_tool_outputs = end.client_outputs;
        if is_tool_execution_allowed(&end.finish_reason) && !end.queued.is_empty() {
            let queued = std::mem::take(&mut end.queued);
            client_tool_outputs += self.execute_tools(&attempt, queued, cancellation).await?;
        }
        track_deferred(
            &end.tool_calls,
            &end.result_ids,
            &self.ctx.execution_tools,
            &mut self.pending_deferred,
        );

        let mut performance = end.performance;
        performance.step_time = step_started.elapsed();
        self.emitter
            .send(StreamEvent::FinishStep {
                step_number,
                finish_reason: end.finish_reason,
                usage: end.usage,
                response: StepResponse {
                    id: end.response.id,
                    timestamp: end.response.timestamp,
                    model_id: end.response.model_id,
                    headers: end.response.headers,
                    body: None,
                    messages: Vec::new(),
                },
                provider_metadata: end.provider_metadata,
                performance,
            })
            .await?;
        Ok(LoopState {
            client_tool_calls,
            client_tool_outputs,
            denied_approvals: end.denied,
            pending_deferred: self.pending_deferred.len(),
        })
    }

    /// Executes `queued` concurrently, forwarding preliminary and final
    /// results as they arrive. Returns the number of final outputs.
    async fn execute_tools(
        &mut self,
        attempt: &Attempt,
        queued: Vec<ParsedToolCall>,
        cancellation: &CallCancellation,
    ) -> Result<usize, Error> {
        let ctx = Arc::clone(&self.ctx);
        let messages = Arc::clone(&attempt.step_messages);
        let (progress_tx, mut progress_rx) =
            mpsc::channel::<StepContent>(TOOL_RESULT_CHANNEL_CAPACITY);
        let mut progress_tx = Some(progress_tx);
        let mut tasks: JoinSet<Result<StepContent, Error>> = JoinSet::new();
        let mut pending = queued.into_iter().filter_map(|call| {
            let tool = ctx.execution_tools.get(call.tool_name.as_str())?;
            tool.is_executable().then(|| (call, Arc::clone(tool)))
        });
        let max = ctx.config.max_tool_concurrency.unwrap_or(usize::MAX);
        let mut spawn_next = |tasks: &mut JoinSet<Result<StepContent, Error>>,
                              progress_tx: &mut Option<mpsc::Sender<StepContent>>|
         -> Result<bool, Error> {
            let Some((call, tool)) = pending.next() else {
                *progress_tx = None;
                return Ok(false);
            };
            let task = ctx.tool_task(
                &tool,
                &call,
                &messages,
                ToolEnvironment::for_step(&attempt.inputs),
                cancellation,
            )?;
            let span = spans::tool_span(call.tool_name.as_str(), call.tool_call_id.as_str());
            let progress = progress_tx.clone();
            tasks.spawn(run_tool_call(call, tool, task, progress).instrument(span));
            Ok(true)
        };
        for _ in 0..max {
            if !spawn_next(&mut tasks, &mut progress_tx)? {
                break;
            }
        }

        let mut outputs = 0;
        loop {
            tokio::select! {
                biased;
                Some(content) = progress_rx.recv() => {
                    if let Some(event) = content_event(content) {
                        self.emitter.send(event).await?;
                    }
                }
                Some(joined) = tasks.join_next() => {
                    let result = joined
                        .map_err(|error| Error::message(format!("tool task failed: {error}")))?;
                    match result {
                        Ok(content) => {
                            outputs += 1;
                            if let Some(event) = content_event(content) {
                                self.emitter.send(event).await?;
                            }
                        }
                        Err(error) => {
                            tasks.abort_all();
                            return Err(cancellation.map_error(error));
                        }
                    }
                    spawn_next(&mut tasks, &mut progress_tx)?;
                }
                else => break,
            }
        }
        Ok(outputs)
    }

    /// Emits the terminal `Abort` or `Error` event for `error`.
    async fn report_failure(&mut self, error: Error) -> StageOutcome {
        if error.is_cancelled() {
            let _ = self.emitter.send(StreamEvent::Abort).await;
            return StageOutcome::Failed(error);
        }
        if !self.error_reported {
            self.ctx
                .telemetry
                .on_error(&ErrorEvent {
                    call_id: &self.ctx.call_id,
                    error: &error,
                    phase: ErrorPhase::Stream,
                })
                .await;
        }
        let _ = self
            .emitter
            .send(StreamEvent::Error {
                error: StreamErrorInfo::from_error(&error),
            })
            .await;
        StageOutcome::Failed(error)
    }
}
