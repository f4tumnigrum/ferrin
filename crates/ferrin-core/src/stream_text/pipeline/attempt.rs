//! Reading one model stream: first-chunk and chunk timeouts, stream-level
//! retries, tool-part buffering and the model-call-end bookkeeping. The
//! mapping of individual parts lives in `parts.rs`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use ferrin_message::Message;
use ferrin_spec::BoxStream;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::language_model::usage::add_token_counts;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use futures_util::StreamExt;
use tokio::time::Instant;
use tokio::time::Sleep;
use tracing::Instrument;

use super::stage::Emitter;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::generate_text::ChunkTimingStats;
use crate::generate_text::ParsedToolCall;
use crate::generate_text::StepContent;
use crate::generate_text::StepPerformance;
use crate::generate_text::StepRequest;
use crate::generate_text::inputs::StepInputs;
use crate::generate_text::inputs::complete_response_metadata;
use crate::generate_text::run::LoopContext;
use crate::hooks::Hooks;
use crate::retry::retry;
use crate::stream_text::StreamErrorInfo;
use crate::stream_text::StreamEvent;
use crate::stream_text::builder::ErrorDecision;
use crate::stream_text::builder::StreamConfig;
use crate::telemetry::ErrorEvent;
use crate::telemetry::ErrorPhase;
use crate::telemetry::ModelCallContext;
use crate::telemetry::ModelCallEndEvent;
use crate::telemetry::ModelCallOutcome;
use crate::telemetry::spans;
use crate::timeout::TimeoutScope;

/// What is known about a tool call whose input is being streamed.
pub(super) struct ToolInputInfo {
    pub(super) tool: Option<Arc<Tool>>,
    pub(super) tool_name: ToolName,
}

/// State of one attempt; replaced when the model call is retried.
pub(super) struct AttemptState {
    pub(super) call_started: Instant,
    pub(super) warnings: Vec<Warning>,
    pub(super) request: RequestMetadata,
    pub(super) response: ResponseMetadata,
    /// Provider part id → reserved (call-unique) id of open text parts.
    pub(super) text_ids: HashMap<PartId, PartId>,
    /// Same for reasoning parts.
    pub(super) reasoning_ids: HashMap<PartId, PartId>,
    /// Reserved ids of text parts whose start was emitted but not their end.
    pub(super) open_text: Vec<PartId>,
    pub(super) open_reasoning: Vec<PartId>,
    /// Model content in arrival order (for the model-call-end event).
    pub(super) content: Vec<StepContent>,
    /// Reserved part id → index into `content`.
    pub(super) part_index: HashMap<PartId, usize>,
    pub(super) tool_inputs: HashMap<ToolCallId, ToolInputInfo>,
    pub(super) tool_calls: Vec<ParsedToolCall>,
    /// Valid client tool calls cleared for execution.
    pub(super) queued: Vec<ParsedToolCall>,
    /// Tool call ids with a provider result in this attempt.
    pub(super) result_ids: HashSet<ToolCallId>,
    /// Client tool outputs emitted while reading (invalid-call errors).
    pub(super) client_outputs: usize,
    /// Automatic approval denials.
    pub(super) denied: usize,
    pub(super) first_output: Option<Instant>,
    pub(super) last_output: Option<Instant>,
    pub(super) gaps: Vec<Duration>,
    /// Tool parts (and everything after them) held back until the model call
    /// ends, while a retry could still discard the attempt.
    pub(super) buffer: Vec<StreamEvent>,
}

impl AttemptState {
    fn new(call_started: Instant) -> Self {
        Self {
            call_started,
            warnings: Vec::new(),
            request: RequestMetadata::default(),
            response: ResponseMetadata::default(),
            text_ids: HashMap::new(),
            reasoning_ids: HashMap::new(),
            open_text: Vec::new(),
            open_reasoning: Vec::new(),
            content: Vec::new(),
            part_index: HashMap::new(),
            tool_inputs: HashMap::new(),
            tool_calls: Vec::new(),
            queued: Vec::new(),
            result_ids: HashSet::new(),
            client_outputs: 0,
            denied: 0,
            first_output: None,
            last_output: None,
            gaps: Vec::new(),
            buffer: Vec::new(),
        }
    }
}

/// What the model call produced, ready for tool execution.
pub(super) struct ModelCallEnd {
    pub(super) finish_reason: FinishReason,
    pub(super) usage: Usage,
    pub(super) provider_metadata: Option<ProviderMetadata>,
    pub(super) response: ResponseMetadata,
    pub(super) performance: StepPerformance,
    pub(super) tool_calls: Vec<ParsedToolCall>,
    pub(super) queued: Vec<ParsedToolCall>,
    pub(super) result_ids: HashSet<ToolCallId>,
    pub(super) client_outputs: usize,
    pub(super) denied: usize,
}

enum AttemptOutcome {
    Finished(Box<ModelCallEnd>),
    Retry,
}

/// The model call of one step, including its retries.
pub(super) struct Attempt {
    pub(super) ctx: Arc<LoopContext>,
    pub(super) stream: Arc<StreamConfig>,
    pub(super) inputs: StepInputs,
    pub(super) cancellation: CallCancellation,
    pub(super) step_messages: Arc<[Message]>,
    /// Whether the `on_error` callback and telemetry already saw the error
    /// that ended the step.
    pub(super) error_reported: bool,
    pub(super) state: AttemptState,
    call_ctx: ModelCallContext,
    attempt: u32,
    automatic_retries: u32,
    callback_retries: u32,
    started_step: bool,
}

impl Attempt {
    pub(super) fn new(
        ctx: Arc<LoopContext>,
        stream: Arc<StreamConfig>,
        inputs: StepInputs,
        cancellation: CallCancellation,
    ) -> Self {
        let step_messages: Arc<[Message]> = Arc::from(inputs.messages.clone());
        let call_ctx = ModelCallContext {
            call_id: ctx.call_id.clone(),
            step_number: inputs.step_number,
            model: inputs.identity.clone(),
            function_id: ctx.function_id().map(str::to_owned),
        };
        Self {
            ctx,
            stream,
            inputs,
            cancellation,
            step_messages,
            error_reported: false,
            state: AttemptState::new(Instant::now()),
            call_ctx,
            attempt: 0,
            automatic_retries: 0,
            callback_retries: 0,
            started_step: false,
        }
    }

    fn retries_possible(&self) -> bool {
        match self.stream.stream_retries {
            Some(0) => self.stream.on_error.is_some(),
            Some(_) => true,
            None => false,
        }
    }

    /// The execution context of a tool call in this step.
    pub(super) fn tool_context_for(
        &self,
        tool: &Tool,
        tool_call_id: &ToolCallId,
        tool_name: &ToolName,
    ) -> Result<ToolContext, Error> {
        self.ctx.tool_context(
            tool,
            tool_call_id,
            tool_name,
            &self.step_messages,
            self.inputs.tools_context.as_ref(),
            &self.cancellation,
        )
    }

    /// Calls the model (with request-level retries) and resets the attempt
    /// state.
    pub(super) async fn call_model(&mut self) -> Result<BoxStream<'static, StreamPart>, Error> {
        let ctx = &self.ctx;
        let inputs = &self.inputs;
        let call_ctx = &self.call_ctx;
        let cancellation = &self.cancellation;
        let (started, result, contract) =
            retry(&ctx.config.retry_policy, cancellation.token(), |_attempt| {
                let options = inputs.options.clone();
                let model = Arc::clone(&inputs.model);
                let telemetry = &ctx.telemetry;
                async move {
                    let started = Instant::now();
                    let contract = crate::middleware::tool_contract::ToolContract::new(&options);
                    let outcome = contract
                        .scope(async {
                            telemetry
                                .execute_language_model_call(
                                    call_ctx,
                                    Box::pin(async move {
                                        model
                                            .do_stream(options)
                                            .await
                                            .map(|result| {
                                                ModelCallOutcome::Stream(Box::new(result))
                                            })
                                            .map_err(Error::from)
                                    }),
                                )
                                .await
                        })
                        .await;
                    match outcome? {
                        ModelCallOutcome::Stream(result) => Ok((started, *result, contract)),
                        #[allow(
                            unreachable_patterns,
                            reason = "the outcome enum is non-exhaustive"
                        )]
                        _ => Err(Error::message(
                            "telemetry integration returned a generate result for a stream call",
                        )),
                    }
                }
            })
            .await
            .map_err(|error| cancellation.map_error(error))?;
        self.inputs.tool_contract = Some(contract);
        self.inputs.refresh_tools(&self.ctx.model_tools);
        self.state = AttemptState::new(started);
        self.state.request = result.request;
        self.state.response = result.response;
        Ok(result.stream)
    }

    /// Reads `stream` up to its finish part, retrying the model call on
    /// stream errors when allowed.
    pub(super) async fn read(
        &mut self,
        emitter: &mut Emitter,
        mut stream: BoxStream<'static, StreamPart>,
        mut span: tracing::Span,
        used_ids: &mut HashSet<PartId>,
    ) -> Result<ModelCallEnd, Error> {
        let mut pending_boundary = false;
        loop {
            let outcome = self
                .read_attempt(emitter, &mut stream, used_ids, pending_boundary)
                .instrument(span.clone())
                .await?;
            match outcome {
                AttemptOutcome::Finished(end) => return Ok(*end),
                AttemptOutcome::Retry => {
                    span = spans::model_call_span(&self.inputs.identity);
                    stream = self.call_model().instrument(span.clone()).await?;
                    pending_boundary = true;
                }
            }
        }
    }

    async fn read_attempt(
        &mut self,
        emitter: &mut Emitter,
        stream: &mut BoxStream<'static, StreamPart>,
        used_ids: &mut HashSet<PartId>,
        mut pending_boundary: bool,
    ) -> Result<AttemptOutcome, Error> {
        let first_chunk = self.ctx.config.timeout.first_chunk;
        let chunk_timeout = self.ctx.config.timeout.chunk;
        let mut deadline: Option<Pin<Box<Sleep>>> =
            first_chunk.map(|duration| Box::pin(tokio::time::sleep(duration)));
        loop {
            let part = tokio::select! {
                biased;
                () = self.cancellation.token().cancelled() => return Err(Error::Cancelled),
                () = async {
                    match deadline.as_mut() {
                        Some(sleep) => sleep.await,
                        None => std::future::pending::<()>().await,
                    }
                } => return Err(self.timeout_error()),
                part = stream.next() => part,
            };
            let Some(part) = part else {
                if pending_boundary {
                    self.emit_boundary(emitter, Vec::new()).await?;
                }
                self.flush_buffer(emitter).await?;
                return Err(Error::invalid_stream_part(
                    "the model stream ended without a finish part",
                ));
            };
            if pending_boundary {
                let warnings = match &part {
                    StreamPart::StreamStart { warnings } => warnings.clone(),
                    _ => Vec::new(),
                };
                self.emit_boundary(emitter, warnings).await?;
                pending_boundary = false;
            }
            if let StreamPart::StreamStart { warnings } = part {
                self.state.warnings = warnings;
                continue;
            }
            if !self.started_step {
                self.emit_start_step(emitter).await?;
            }
            if is_output_chunk(&part) {
                self.record_output_chunk(&mut deadline, chunk_timeout);
            }
            match part {
                StreamPart::Error { error } => {
                    return self.handle_stream_error(emitter, error).await;
                }
                StreamPart::Finish {
                    finish_reason,
                    usage,
                    provider_metadata,
                } => {
                    self.flush_buffer(emitter).await?;
                    let end = self.finish(finish_reason, usage, provider_metadata).await?;
                    return Ok(AttemptOutcome::Finished(Box::new(end)));
                }
                other => {
                    for event in self.map_part(other, used_ids).await? {
                        self.emit(emitter, event).await?;
                    }
                }
            }
        }
    }

    fn step_request(&self) -> StepRequest {
        let include = self.ctx.config.include;
        StepRequest {
            body: include
                .request_body
                .then(|| self.state.request.body.clone())
                .flatten(),
            messages: include
                .request_messages
                .then(|| self.inputs.messages.clone()),
        }
    }

    async fn emit_start_step(&mut self, emitter: &mut Emitter) -> Result<(), Error> {
        self.started_step = true;
        emitter
            .send(StreamEvent::StartStep {
                step_number: self.inputs.step_number,
                runtime_context: self.inputs.runtime_context.clone(),
                tools_context: self.inputs.tools_context.clone(),
                model: self.inputs.identity.clone(),
                request: self.step_request(),
                warnings: self.state.warnings.clone(),
            })
            .await
    }

    async fn emit_boundary(
        &mut self,
        emitter: &mut Emitter,
        warnings: Vec<Warning>,
    ) -> Result<(), Error> {
        self.state.warnings = warnings.clone();
        emitter
            .send(StreamEvent::RetryAttempt {
                step_number: self.inputs.step_number,
                attempt: self.attempt,
                request: self.step_request(),
                warnings,
            })
            .await
    }

    fn record_output_chunk(
        &mut self,
        deadline: &mut Option<Pin<Box<Sleep>>>,
        chunk_timeout: Option<Duration>,
    ) {
        let now = Instant::now();
        if let Some(last) = self.state.last_output {
            self.state.gaps.push(now.saturating_duration_since(last));
        }
        if self.state.first_output.is_none() {
            self.state.first_output = Some(now);
            *deadline = chunk_timeout.map(|duration| Box::pin(tokio::time::sleep(duration)));
        } else if let (Some(sleep), Some(duration)) = (deadline.as_mut(), chunk_timeout) {
            sleep.as_mut().reset(now + duration);
        }
        self.state.last_output = Some(now);
    }

    fn timeout_error(&self) -> Error {
        let now = Instant::now();
        let (scope, elapsed) = match self.state.first_output {
            None => (
                TimeoutScope::FirstChunk,
                now.saturating_duration_since(self.state.call_started),
            ),
            Some(_) => (
                TimeoutScope::Chunk,
                self.state
                    .last_output
                    .map_or(Duration::ZERO, |last| now.saturating_duration_since(last)),
            ),
        };
        self.cancellation.cancel_for_timeout(scope.clone(), elapsed);
        Error::Timeout { scope, elapsed }
    }

    /// Emits `event`, holding tool parts back while a retry is still possible.
    async fn emit(&mut self, emitter: &mut Emitter, event: StreamEvent) -> Result<(), Error> {
        let is_tool_part = matches!(
            event,
            StreamEvent::ToolInputStart { .. }
                | StreamEvent::ToolInputDelta { .. }
                | StreamEvent::ToolInputEnd { .. }
                | StreamEvent::ToolCall(_)
                | StreamEvent::ToolApprovalRequest(_)
                | StreamEvent::ToolApprovalResponse(_)
                | StreamEvent::ToolResult(_)
                | StreamEvent::ToolError(_)
        );
        if self.retries_possible() && (is_tool_part || !self.state.buffer.is_empty()) {
            self.state.buffer.push(event);
            return Ok(());
        }
        self.send_tracked(emitter, event).await
    }

    async fn send_tracked(
        &mut self,
        emitter: &mut Emitter,
        event: StreamEvent,
    ) -> Result<(), Error> {
        match &event {
            StreamEvent::TextStart { id, .. } => self.state.open_text.push(id.clone()),
            StreamEvent::TextEnd { id, .. } => self.state.open_text.retain(|open| open != id),
            StreamEvent::ReasoningStart { id, .. } => self.state.open_reasoning.push(id.clone()),
            StreamEvent::ReasoningEnd { id, .. } => {
                self.state.open_reasoning.retain(|open| open != id);
            }
            _ => {}
        }
        emitter.send(event).await
    }

    async fn flush_buffer(&mut self, emitter: &mut Emitter) -> Result<(), Error> {
        for event in std::mem::take(&mut self.state.buffer) {
            self.send_tracked(emitter, event).await?;
        }
        Ok(())
    }

    async fn close_open_parts(&mut self, emitter: &mut Emitter) -> Result<(), Error> {
        for id in std::mem::take(&mut self.state.open_text) {
            emitter
                .send(StreamEvent::TextEnd {
                    id,
                    provider_metadata: None,
                })
                .await?;
        }
        for id in std::mem::take(&mut self.state.open_reasoning) {
            emitter
                .send(StreamEvent::ReasoningEnd {
                    id,
                    provider_metadata: None,
                })
                .await?;
        }
        Ok(())
    }

    /// Handles an error part: retries the model call when allowed, otherwise
    /// fails the step.
    async fn handle_stream_error(
        &mut self,
        emitter: &mut Emitter,
        error: StreamError,
    ) -> Result<AttemptOutcome, Error> {
        let error = Error::stream(error);
        self.ctx.telemetry.on_error(&ErrorEvent {
            call_id: &self.ctx.call_id,
            error: &error,
            phase: ErrorPhase::Stream,
        });
        let decision = match &self.stream.on_error {
            Some(callback) => callback.call(StreamErrorInfo::from_error(&error)).await,
            None => ErrorDecision::Continue,
        };
        self.error_reported = true;

        let retries = self.stream.stream_retries;
        let automatic = retries.is_some_and(|max| self.automatic_retries < max);
        let callback_retry = !automatic
            && retries.is_some()
            && self.stream.on_error.is_some()
            && matches!(decision, ErrorDecision::Retry)
            && self.callback_retries < 1;
        if !automatic && !callback_retry {
            self.flush_buffer(emitter).await?;
            return Err(error);
        }
        if automatic {
            self.automatic_retries += 1;
        } else {
            self.callback_retries += 1;
        }
        tracing::info!(
            target: "ferrin::stream_text",
            step_number = self.inputs.step_number,
            attempt = self.attempt + 1,
            error = %error,
            "retrying the model call after a stream error"
        );
        self.state.buffer.clear();
        self.close_open_parts(emitter).await?;
        self.attempt += 1;
        Ok(AttemptOutcome::Retry)
    }

    /// Model-call-end bookkeeping: performance, response metadata, the
    /// model-call-end event and tool choice enforcement.
    async fn finish(
        &mut self,
        finish_reason: FinishReason,
        usage: Usage,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Result<ModelCallEnd, Error> {
        let now = Instant::now();
        let response_time = now.saturating_duration_since(self.state.call_started);
        let time_to_first_output = self
            .state
            .first_output
            .map(|first| first.saturating_duration_since(self.state.call_started));
        let output_tokens = usage.output.total;
        let input_tokens = usage.input.total;
        let performance = StepPerformance {
            step_time: Duration::ZERO,
            response_time,
            time_to_first_output,
            output_tokens_per_second: time_to_first_output.map(|first| {
                StepPerformance::tokens_per_second(
                    output_tokens,
                    response_time.saturating_sub(first),
                )
            }),
            effective_output_tokens_per_second: StepPerformance::tokens_per_second(
                output_tokens,
                response_time,
            ),
            input_tokens_per_second: time_to_first_output
                .map(|first| StepPerformance::tokens_per_second(input_tokens, first)),
            effective_total_tokens_per_second: StepPerformance::tokens_per_second(
                add_token_counts(input_tokens, output_tokens),
                response_time,
            ),
            time_between_output_chunks: ChunkTimingStats::from_gaps(&self.state.gaps),
        };

        complete_response_metadata(&self.ctx, &self.inputs, &mut self.state.response);
        spans::log_warnings(&self.state.warnings, &self.inputs.identity);
        let span = tracing::Span::current();
        if let Some(id) = &self.state.response.id {
            span.record("gen_ai.response.id", id.as_str());
        }
        span.record(
            "gen_ai.response.finish_reasons",
            tracing::field::debug(&finish_reason.unified),
        );
        if let Some(tokens) = input_tokens {
            span.record("gen_ai.usage.input_tokens", tokens);
        }
        if let Some(tokens) = output_tokens {
            span.record("gen_ai.usage.output_tokens", tokens);
        }
        if let Some(first) = time_to_first_output {
            span.record(
                "ferrin.time_to_first_output_ms",
                u64::try_from(first.as_millis()).unwrap_or(u64::MAX),
            );
        }

        let event = Arc::new(ModelCallEndEvent {
            runtime_context: self.inputs.runtime_context.clone(),
            call_id: self.ctx.call_id.clone(),
            step_number: self.inputs.step_number,
            model: self.inputs.identity.clone(),
            content: self
                .ctx
                .telemetry
                .record_outputs()
                .then(|| self.state.content.clone()),
            finish_reason: finish_reason.clone(),
            usage: usage.clone(),
            response: self.state.response.clone(),
            performance: performance.clone(),
            warnings: self.state.warnings.clone(),
        });
        self.ctx.telemetry.on_language_model_call_end(&event);
        Hooks::emit(&self.ctx.hooks.on_language_model_call_end, event).await;

        self.check_tool_choice()?;
        Ok(ModelCallEnd {
            finish_reason,
            usage,
            provider_metadata,
            response: self.state.response.clone(),
            performance,
            tool_calls: std::mem::take(&mut self.state.tool_calls),
            queued: std::mem::take(&mut self.state.queued),
            result_ids: std::mem::take(&mut self.state.result_ids),
            client_outputs: self.state.client_outputs,
            denied: self.state.denied,
        })
    }

    fn check_tool_choice(&mut self) -> Result<(), Error> {
        self.inputs.refresh_tools(&self.ctx.model_tools);
        crate::generate_text::parse_tool_call::check_tool_choice(
            self.inputs.tool_choice.as_ref(),
            &self.state.tool_calls,
        )
    }
}

/// Whether `part` counts as model output for timeouts and performance.
fn is_output_chunk(part: &StreamPart) -> bool {
    match part {
        StreamPart::TextDelta { delta, .. }
        | StreamPart::ReasoningDelta { delta, .. }
        | StreamPart::ToolInputDelta { delta, .. } => !delta.is_empty(),
        StreamPart::File { .. } | StreamPart::ReasoningFile { .. } | StreamPart::ToolCall(_) => {
            true
        }
        _ => false,
    }
}
