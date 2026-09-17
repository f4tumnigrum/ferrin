//! The event processor: the consumer-side stage that accumulates step
//! results from the (transformed) events, runs the hooks and resolves the
//! completion.

use std::collections::HashMap;
use std::sync::Arc;

use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::stream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use super::stage::StageOutcome;
use crate::error::Error;
use crate::generate_text::GenerateTextResult;
use crate::generate_text::StepContent;
use crate::generate_text::StepRequest;
use crate::generate_text::StepResponse;
use crate::generate_text::StepResult;
use crate::generate_text::replay::replay_tool_message;
use crate::generate_text::response_messages::to_response_messages;
use crate::generate_text::run::LoopContext;
use crate::generate_text::run::parse_output;
use crate::hooks::Hooks;
use crate::output::OutputHandler;
use crate::stream_text::EventStream;
use crate::stream_text::StreamEvent;
use crate::stream_text::TransformContext;
use crate::stream_text::builder::StreamConfig;
use crate::telemetry::AbortEvent;
use crate::telemetry::ErrorEvent;
use crate::telemetry::ErrorPhase;
use crate::telemetry::ModelIdentity;

/// Consumer-side state of the pipeline.
pub(super) struct Processor<O> {
    ctx: Arc<LoopContext>,
    output: Arc<dyn OutputHandler<O>>,
    /// The producer task; dropping the processor aborts it.
    _tasks: JoinSet<()>,
    step_tx: mpsc::Sender<StepResult>,
    outcome_rx: Option<oneshot::Receiver<StageOutcome>>,
    completion_tx: Option<oneshot::Sender<Result<GenerateTextResult<O>, Error>>>,
    replay: Vec<StepContent>,
    steps: Vec<StepResult>,
    current: Option<StepAccumulator>,
    total_usage: Usage,
    aborted: bool,
    transform_context: Option<TransformContext>,
    stream_config: Option<Arc<StreamConfig>>,
}

/// Content of the step in progress.
struct StepAccumulator {
    runtime_context: Option<ferrin_spec::JsonValue>,
    tools_context: Option<ferrin_spec::JsonValue>,
    model: ModelIdentity,
    request: StepRequest,
    warnings: Vec<Warning>,
    content: Vec<StepContent>,
    text_parts: HashMap<PartId, usize>,
    reasoning_parts: HashMap<PartId, usize>,
}

impl StepAccumulator {
    fn reset(&mut self, request: StepRequest, warnings: Vec<Warning>) {
        self.request = request;
        self.warnings = warnings;
        self.content.clear();
        self.text_parts.clear();
        self.reasoning_parts.clear();
    }

    fn start_part(&mut self, reasoning: bool, id: &PartId, metadata: Option<ProviderMetadata>) {
        let index = self.content.len();
        self.content.push(if reasoning {
            StepContent::Reasoning {
                text: String::new(),
                provider_metadata: metadata,
            }
        } else {
            StepContent::Text {
                text: String::new(),
                provider_metadata: metadata,
            }
        });
        let parts = if reasoning {
            &mut self.reasoning_parts
        } else {
            &mut self.text_parts
        };
        parts.insert(id.clone(), index);
    }

    fn update_part(
        &mut self,
        reasoning: bool,
        id: &PartId,
        delta: Option<&str>,
        metadata: Option<&ProviderMetadata>,
        end: bool,
    ) -> Result<(), Error> {
        let kind = if reasoning { "reasoning" } else { "text" };
        let parts = if reasoning {
            &mut self.reasoning_parts
        } else {
            &mut self.text_parts
        };
        let index = if end {
            parts.remove(id)
        } else {
            parts.get(id).copied()
        }
        .ok_or_else(|| Error::invalid_stream_part(format!("{kind} part `{id}` not found")))?;
        match self.content.get_mut(index) {
            Some(
                StepContent::Text {
                    text,
                    provider_metadata,
                }
                | StepContent::Reasoning {
                    text,
                    provider_metadata,
                },
            ) => {
                if let Some(delta) = delta {
                    text.push_str(delta);
                }
                if metadata.is_some() {
                    *provider_metadata = metadata.cloned();
                }
                Ok(())
            }
            _ => Err(Error::invalid_stream_part(format!(
                "{kind} part `{id}` does not refer to a {kind} part"
            ))),
        }
    }
}

impl<O: Send + 'static> Processor<O> {
    pub(super) fn new(
        ctx: Arc<LoopContext>,
        output: Arc<dyn OutputHandler<O>>,
        tasks: JoinSet<()>,
        step_tx: mpsc::Sender<StepResult>,
        outcome_rx: oneshot::Receiver<StageOutcome>,
        completion_tx: oneshot::Sender<Result<GenerateTextResult<O>, Error>>,
    ) -> Self {
        Self {
            ctx,
            output,
            _tasks: tasks,
            step_tx,
            outcome_rx: Some(outcome_rx),
            completion_tx: Some(completion_tx),
            replay: Vec::new(),
            steps: Vec::new(),
            current: None,
            total_usage: Usage::default(),
            aborted: false,
            transform_context: None,
            stream_config: None,
        }
    }

    pub(super) fn with_transform_context(
        mut self,
        context: TransformContext,
        stream: Arc<StreamConfig>,
    ) -> Self {
        self.transform_context = Some(context);
        self.stream_config = Some(stream);
        self
    }

    fn push_content(&mut self, content: StepContent) {
        match &mut self.current {
            Some(step) => step.content.push(content),
            None => self.replay.push(content),
        }
    }

    fn current_step(&mut self) -> Result<&mut StepAccumulator, Error> {
        self.current
            .as_mut()
            .ok_or_else(|| Error::invalid_stream_part("content event outside a step"))
    }

    async fn handle(&mut self, event: &StreamEvent) -> Result<(), Error> {
        match event {
            StreamEvent::StartStep {
                model,
                runtime_context,
                tools_context,
                request,
                warnings,
                ..
            } => {
                self.current = Some(StepAccumulator {
                    runtime_context: runtime_context.clone(),
                    tools_context: tools_context.clone(),
                    model: model.clone(),
                    request: request.clone(),
                    warnings: warnings.clone(),
                    content: Vec::new(),
                    text_parts: HashMap::new(),
                    reasoning_parts: HashMap::new(),
                });
            }
            StreamEvent::RetryAttempt {
                request, warnings, ..
            } => {
                if let Some(step) = &mut self.current {
                    step.reset(request.clone(), warnings.clone());
                }
            }
            StreamEvent::TextStart {
                id,
                provider_metadata,
            } => self
                .current_step()?
                .start_part(false, id, provider_metadata.clone()),
            StreamEvent::TextDelta {
                id,
                text,
                provider_metadata,
            } => self.current_step()?.update_part(
                false,
                id,
                Some(text),
                provider_metadata.as_ref(),
                false,
            )?,
            StreamEvent::TextEnd {
                id,
                provider_metadata,
            } => self.current_step()?.update_part(
                false,
                id,
                None,
                provider_metadata.as_ref(),
                true,
            )?,
            StreamEvent::ReasoningStart {
                id,
                provider_metadata,
            } => self
                .current_step()?
                .start_part(true, id, provider_metadata.clone()),
            StreamEvent::ReasoningDelta {
                id,
                text,
                provider_metadata,
            } => self.current_step()?.update_part(
                true,
                id,
                Some(text),
                provider_metadata.as_ref(),
                false,
            )?,
            StreamEvent::ReasoningEnd {
                id,
                provider_metadata,
            } => self.current_step()?.update_part(
                true,
                id,
                None,
                provider_metadata.as_ref(),
                true,
            )?,
            StreamEvent::ReasoningFile(file) => {
                self.push_content(StepContent::ReasoningFile(file.clone()));
            }
            StreamEvent::File(file) => self.push_content(StepContent::File(file.clone())),
            StreamEvent::Source(source) => {
                self.push_content(StepContent::Source(source.clone()));
            }
            StreamEvent::Custom {
                kind,
                provider_metadata,
            } => self.push_content(StepContent::Custom {
                kind: kind.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            StreamEvent::ToolCall(call) => self.push_content(StepContent::ToolCall(call.clone())),
            StreamEvent::ToolResult(result) => {
                if !result.preliminary {
                    self.push_content(StepContent::ToolResult(result.clone()));
                }
            }
            StreamEvent::ToolError(error) => {
                self.push_content(StepContent::ToolError(error.clone()));
            }
            StreamEvent::ToolApprovalRequest(request) => {
                self.push_content(StepContent::ToolApprovalRequest(request.clone()));
            }
            StreamEvent::ToolApprovalResponse(response) => {
                self.push_content(StepContent::ToolApprovalResponse(response.clone()));
            }
            StreamEvent::ToolOutputDenied(denied) => {
                self.push_content(StepContent::ToolOutputDenied(denied.clone()));
            }
            StreamEvent::FinishStep {
                step_number,
                finish_reason,
                usage,
                response,
                provider_metadata,
                performance,
            } => {
                let step = self
                    .current
                    .take()
                    .ok_or_else(|| Error::invalid_stream_part("finish-step without start-step"))?;
                let mut messages = to_response_messages(&step.content, &self.ctx.config.tools);
                if self.steps.is_empty()
                    && let Some(replay) =
                        replay_tool_message(&self.replay, &self.ctx.execution_tools)
                {
                    messages.insert(0, replay);
                }
                let result = StepResult {
                    step_number: *step_number,
                    runtime_context: step.runtime_context,
                    tools_context: step.tools_context,
                    model: step.model,
                    content: step.content,
                    finish_reason: finish_reason.clone(),
                    usage: usage.clone(),
                    warnings: step.warnings,
                    request: step.request,
                    response: StepResponse {
                        messages,
                        ..response.clone()
                    },
                    provider_metadata: provider_metadata.clone(),
                    performance: performance.clone(),
                };
                let result = self.ctx.emit_step_end(result).await;
                self.total_usage = self.total_usage.add(&result.usage);
                self.steps.push(result.clone());
                // The producer waits for the step before deciding whether to
                // continue; a closed channel means it is gone already.
                let _ = self.step_tx.send(result).await;
            }
            StreamEvent::Abort => {
                self.aborted = true;
                self.emit_abort().await;
            }
            StreamEvent::Start { .. }
            | StreamEvent::ToolInputStart { .. }
            | StreamEvent::ToolInputDelta { .. }
            | StreamEvent::ToolInputEnd { .. }
            | StreamEvent::Finish { .. }
            | StreamEvent::Error { .. }
            | StreamEvent::Raw { .. } => {}
        }
        Ok(())
    }

    async fn emit_abort(&self) {
        let event = Arc::new(AbortEvent {
            call_id: self.ctx.call_id.clone(),
            steps_completed: u32::try_from(self.steps.len()).unwrap_or(u32::MAX),
        });
        self.ctx.telemetry.on_abort(&event).await;
        Hooks::emit(&self.ctx.hooks.on_abort, event).await;
    }

    fn complete(&mut self, result: Result<GenerateTextResult<O>, Error>) {
        if let Some(sender) = self.completion_tx.take() {
            let _ = sender.send(result);
        }
    }

    /// Finalizes after the (transformed) stream ended.
    async fn finish(&mut self) {
        if let Some(error) = self
            .transform_context
            .as_ref()
            .and_then(TransformContext::take_failure)
        {
            self.fail(error).await;
            return;
        }
        // The producer can finish while a transform is still buffering output.
        // Cancellation during that drain must not turn into partial success.
        if self.ctx.cancellation.is_cancelled() {
            self._tasks.abort_all();
            let error = self.ctx.cancellation.error();
            if error.is_cancelled() && !self.aborted {
                self.emit_abort().await;
            }
            self.complete(Err(error));
            return;
        }
        let outcome = match self.outcome_rx.take().map(|mut rx| rx.try_recv()) {
            Some(Ok(outcome)) => outcome,
            // The producer is still running: the stream was cut short (by a
            // transform or by dropping the events early).
            Some(Err(oneshot::error::TryRecvError::Empty)) => {
                self._tasks.abort_all();
                StageOutcome::Failed(if self.ctx.cancellation.is_cancelled() {
                    self.ctx.cancellation.error()
                } else {
                    Error::Cancelled
                })
            }
            Some(Err(oneshot::error::TryRecvError::Closed)) | None => {
                StageOutcome::Failed(Error::Cancelled)
            }
        };
        match outcome {
            StageOutcome::Failed(error) => {
                if error.is_cancelled() && !self.aborted {
                    self.emit_abort().await;
                }
                self.complete(Err(error));
            }
            StageOutcome::Finished => {
                if self.steps.is_empty() {
                    self.complete(Err(Error::NoOutputGenerated));
                    return;
                }
                self.ctx.emit_end(&self.steps, &self.total_usage).await;
                let output = parse_output(self.output.as_ref(), &self.steps);
                let result = match output {
                    Ok(output) => Ok(GenerateTextResult {
                        steps: std::mem::take(&mut self.steps),
                        total_usage: self.total_usage.clone(),
                        output,
                    }),
                    Err(error) => {
                        self.ctx
                            .telemetry
                            .on_error(&ErrorEvent {
                                call_id: &self.ctx.call_id,
                                error: &error,
                                phase: ErrorPhase::Output,
                            })
                            .await;
                        Err(error)
                    }
                };
                self.complete(result);
            }
        }
    }

    async fn fail(&mut self, error: Error) {
        self._tasks.abort_all();
        self.ctx
            .telemetry
            .on_error(&ErrorEvent {
                call_id: &self.ctx.call_id,
                error: &error,
                phase: ErrorPhase::Stream,
            })
            .await;
        self.complete(Err(error));
    }
}

/// Wraps `events` so that each event updates the processor before it is
/// forwarded; the completion resolves when the stream ends.
pub(super) fn process<O: Send + 'static>(
    events: EventStream,
    processor: Processor<O>,
) -> impl Stream<Item = StreamEvent> + Send {
    stream::unfold(
        (events, processor),
        |(mut events, mut processor)| async move {
            match events.next().await {
                Some(event) => {
                    let handled = match &event {
                        StreamEvent::Error { error } => processor
                            .stream_config
                            .as_ref()
                            .is_some_and(|stream| stream.take_error_handled(error)),
                        StreamEvent::RetryAttempt { .. } => true,
                        _ => false,
                    };
                    if !handled {
                        Hooks::emit(&processor.ctx.hooks.on_chunk, Arc::new(event.clone())).await;
                        if let StreamEvent::Error { error } = &event
                            && let Some(stream) = &processor.stream_config
                        {
                            let _ = stream.error_decision(error.clone()).await;
                        }
                    }
                    match processor.handle(&event).await {
                        Ok(()) => Some((event, (events, processor))),
                        Err(error) => {
                            processor.fail(error).await;
                            None
                        }
                    }
                }
                None => {
                    processor.finish().await;
                    None
                }
            }
        },
    )
}
