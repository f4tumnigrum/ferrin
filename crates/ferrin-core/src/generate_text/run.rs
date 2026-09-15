//! The generation loop.
//!
//! [`LoopContext`] holds the per-call state shared with the streaming
//! pipeline (approval replay, tool execution, event emission); [`run`] drives
//! the non-streaming loop.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use super::GenerateTextResult;
use super::ParsedToolCall;
use super::StepContent;
use super::StepPerformance;
use super::StepRequest;
use super::StepResponse;
use super::StepResult;
use super::StopCondition;
use super::ToolApprovalRequestContent;
use super::ToolErrorInfo;
use super::ToolExecutionError;
use super::ToolResult;
use super::config::CallConfig;
use super::inputs::complete_response_metadata;
use super::inputs::emit_model_call_start;
use super::inputs::prepare_step_inputs;
use super::parse_tool_call::ParseContext;
use super::parse_tool_call::parse_tool_call;
use super::replay::replay_approvals;
use super::replay::replay_tool_message;
use super::response_messages::to_response_messages;
use super::step_count;
use super::stop_condition::is_stop_condition_met;
use super::tools::ToolTask;
use super::tools::execute_tools;
use super::tools::invalid_tool_errors;
use super::tools::resolve_approvals;
use super::tools::track_deferred;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::hooks::Hooks;
use crate::output::OutputContext;
use crate::output::OutputHandler;
use crate::prompt::Instructions;
use crate::prompt::standardize;
use crate::registry::resolve_language_model;
use crate::retry::retry;
use crate::telemetry::EndEvent;
use crate::telemetry::ModelCallContext;
use crate::telemetry::ModelCallEndEvent;
use crate::telemetry::ModelCallOutcome;
use crate::telemetry::ModelIdentity;
use crate::telemetry::RecordedInputs;
use crate::telemetry::StartEvent;
use crate::telemetry::StepEndEvent;
use crate::telemetry::TelemetryDispatcher;
use crate::telemetry::spans;
use crate::timeout::TimeoutScope;
use ferrin_message::Message;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use ferrin_spec::language_model::usage::add_token_counts;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolSet;
use ferrin_tool::callers::prepare_tools_for_callers;
use ferrin_tool::callers::validate_tool_callers;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::Instant;
use tracing::Instrument;

/// Continuation bookkeeping of one step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LoopState {
    /// Tool calls the client must handle (not provider-executed).
    pub(crate) client_tool_calls: usize,
    /// Client tool outputs produced in the step (results and errors).
    pub(crate) client_tool_outputs: usize,
    /// Approval decisions that denied execution.
    pub(crate) denied_approvals: usize,
    /// Provider tool calls whose results are still deferred.
    pub(crate) pending_deferred: usize,
}

impl LoopState {
    /// Whether the loop may run another step (before stop conditions).
    #[must_use]
    pub(crate) fn should_continue(&self) -> bool {
        self.client_tool_outputs + self.denied_approvals == self.client_tool_calls
            && (self.client_tool_calls > 0 || self.pending_deferred > 0)
    }
}

/// Whether tools execute after a model call with `finish_reason`.
#[must_use]
pub(crate) fn is_tool_execution_allowed(finish_reason: &FinishReason) -> bool {
    matches!(
        finish_reason.unified,
        FinishReasonKind::Stop | FinishReasonKind::ToolCalls
    )
}

/// Per-call state shared by the generate and stream loops.
pub(crate) struct LoopContext {
    pub(crate) config: Arc<CallConfig>,
    pub(crate) telemetry: TelemetryDispatcher,
    pub(crate) hooks: Arc<Hooks>,
    pub(crate) call_id: String,
    pub(crate) model: Arc<dyn DynLanguageModel>,
    pub(crate) identity: ModelIdentity,
    pub(crate) execution_tools: Arc<ToolSet>,
    pub(crate) model_tools: ToolSet,
    pub(crate) instructions: Option<Instructions>,
    pub(crate) initial_messages: Vec<Message>,
    pub(crate) response_format: Option<ResponseFormat>,
    pub(crate) cancellation: CallCancellation,
}

impl LoopContext {
    /// Validates the configuration and resolves the model.
    pub(crate) fn new(
        config: CallConfig,
        response_format: Option<ResponseFormat>,
    ) -> Result<Self, Error> {
        config.settings.validate()?;
        let model = resolve_language_model(&config.model)?;
        let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
        let standardized = standardize(
            config.system.clone(),
            config.prompt.clone(),
            config.messages.clone(),
            config.allow_system_in_messages,
        )?;
        validate_tool_callers(&config.tools, &config.tool_callers)?;
        let prepared = prepare_tools_for_callers(&config.tools, &config.tool_callers);
        let telemetry = TelemetryDispatcher::new(config.telemetry.clone());
        let hooks = Arc::new(config.hooks.clone());
        let call_id = config.id_generator.generate();
        let cancellation = CallCancellation::new(&config.cancellation);
        Ok(Self {
            config: Arc::new(config),
            telemetry,
            hooks,
            call_id,
            model,
            identity,
            execution_tools: Arc::new(prepared.execution_tools),
            model_tools: prepared.model_tools,
            instructions: standardized.system,
            initial_messages: standardized.messages,
            response_format,
            cancellation,
        })
    }

    pub(crate) fn function_id(&self) -> Option<&str> {
        self.config.telemetry.function_id.as_deref()
    }

    /// Emits the start event.
    pub(crate) async fn emit_start(&self) {
        let inputs = self.telemetry.record_inputs().then(|| RecordedInputs {
            system: self
                .instructions
                .as_ref()
                .map(|instructions| instructions.content.clone()),
            messages: Arc::from(self.initial_messages.clone()),
        });
        let event = Arc::new(StartEvent {
            call_id: self.call_id.clone(),
            function_id: self.function_id().map(str::to_owned),
            model: self.identity.clone(),
            inputs,
            metadata: self.config.telemetry.metadata.clone(),
        });
        self.telemetry.on_start(&event);
        Hooks::emit(&self.hooks.on_start, event).await;
    }

    /// Emits the end event.
    pub(crate) async fn emit_end(&self, steps: &[StepResult], total_usage: &Usage) {
        let event = Arc::new(EndEvent {
            call_id: self.call_id.clone(),
            steps: Arc::from(steps.to_vec()),
            total_usage: total_usage.clone(),
            output_recorded: None,
        });
        self.telemetry.on_end(&event);
        Hooks::emit(&self.hooks.on_end, event).await;
    }

    /// Emits the step-end event and returns the step.
    pub(crate) async fn emit_step_end(&self, step: StepResult) -> StepResult {
        let shared = Arc::new(step);
        self.telemetry.on_step_end(&StepEndEvent {
            call_id: self.call_id.clone(),
            step: Arc::clone(&shared),
        });
        Hooks::emit(&self.hooks.on_step_end, Arc::clone(&shared)).await;
        Arc::try_unwrap(shared).unwrap_or_else(|shared| (*shared).clone())
    }

    /// The stop conditions of the call (default: one step).
    pub(crate) fn stop_conditions(&self) -> Vec<Arc<dyn StopCondition>> {
        if self.config.stop_conditions.is_empty() {
            vec![Arc::new(step_count(1))]
        } else {
            self.config.stop_conditions.clone()
        }
    }

    /// Builds the execution context of one tool call.
    pub(crate) fn tool_context(
        &self,
        tool: &Tool,
        tool_call_id: &ToolCallId,
        tool_name: &ferrin_spec::ToolName,
        messages: &Arc<[Message]>,
        tools_context: Option<&JsonValue>,
        cancellation: &CallCancellation,
    ) -> ToolContext {
        let validated = tool
            .validate_context(tool_name, tools_context.cloned())
            .ok()
            .flatten();
        let ctx = ToolContext::new(tool_call_id.clone())
            .with_messages(Arc::clone(messages))
            .with_cancellation(cancellation.token().child_token())
            .with_tools_context(validated);
        #[cfg(feature = "sandbox")]
        let ctx = match &self.config.sandbox {
            Some(sandbox) => ctx.with_sandbox(Arc::clone(sandbox)),
            None => ctx,
        };
        ctx
    }

    /// Bundles what one tool task needs.
    pub(crate) fn tool_task(
        &self,
        tool: &Tool,
        call: &ParsedToolCall,
        messages: &Arc<[Message]>,
        tools_context: Option<&JsonValue>,
        cancellation: &CallCancellation,
    ) -> ToolTask {
        ToolTask {
            telemetry: self.telemetry.clone(),
            hooks: Arc::clone(&self.hooks),
            call_id: self.call_id.clone(),
            timeout: self.config.timeout.tool_timeout(&call.tool_name),
            tool_context: self.tool_context(
                tool,
                &call.tool_call_id,
                &call.tool_name,
                messages,
                tools_context,
                cancellation,
            ),
        }
    }
}

/// Runs the non-streaming loop.
pub(crate) async fn run<O: Send + 'static>(
    config: CallConfig,
    output: Arc<dyn OutputHandler<O>>,
) -> Result<GenerateTextResult<O>, Error> {
    let ctx = LoopContext::new(config, output.response_format())?;
    let span = spans::call_span("generate_text", ctx.function_id(), &ctx.identity);
    let total = ctx.config.timeout.total;
    let cancellation = ctx.cancellation.child();
    let result = Box::pin(cancellation.with_timeout(
        TimeoutScope::Total,
        total,
        run_inner(&ctx, output.as_ref()).instrument(span),
    ))
    .await;
    result.map_err(|error| cancellation.map_error(error))
}

async fn run_inner<O: 'static>(
    ctx: &LoopContext,
    output: &dyn OutputHandler<O>,
) -> Result<GenerateTextResult<O>, Error> {
    ctx.emit_start().await;
    let replay = replay_approvals(ctx, &ctx.initial_messages, &ctx.cancellation).await?;
    let replay_message = replay_tool_message(&replay, &ctx.execution_tools);
    let mut response_messages: Vec<Message> = replay_message.iter().cloned().collect();
    let mut steps: Vec<StepResult> = Vec::new();
    let mut pending_deferred: HashSet<ToolCallId> = HashSet::new();
    let stop_conditions = ctx.stop_conditions();

    loop {
        let step_cancellation = ctx.cancellation.child();
        let step_number = u32::try_from(steps.len()).unwrap_or(u32::MAX);
        let (mut step, state) = step_cancellation
            .with_timeout(
                TimeoutScope::Step,
                ctx.config.timeout.step,
                run_step(
                    ctx,
                    &steps,
                    &response_messages,
                    &mut pending_deferred,
                    &step_cancellation,
                )
                .instrument(spans::step_span(step_number)),
            )
            .await
            .map_err(|error| step_cancellation.map_error(error))?;
        response_messages.extend(step.response.messages.iter().cloned());
        if steps.is_empty()
            && let Some(message) = replay_message.clone()
        {
            step.response.messages.insert(0, message);
        }
        let step = ctx.emit_step_end(step).await;
        steps.push(step);
        if !state.should_continue() || is_stop_condition_met(&stop_conditions, &steps).await {
            break;
        }
    }

    let total_usage = steps
        .iter()
        .fold(Usage::default(), |total, step| total.add(&step.usage));
    ctx.emit_end(&steps, &total_usage).await;
    let output = parse_output(output, &steps)?;
    Ok(GenerateTextResult {
        steps,
        total_usage,
        output,
    })
}

/// Parses the structured output of the final step.
pub(crate) fn parse_output<O: 'static>(
    output: &dyn OutputHandler<O>,
    steps: &[StepResult],
) -> Result<O, Error> {
    let Some(last) = steps.last() else {
        return Err(Error::NoOutputGenerated);
    };
    let ctx = OutputContext {
        response: ResponseMetadata {
            id: last.response.id.clone(),
            timestamp: last.response.timestamp,
            model_id: last.response.model_id.clone(),
            headers: last.response.headers.clone(),
            body: last.response.body.clone(),
        },
        usage: last.usage.clone(),
        finish_reason: last.finish_reason.clone(),
    };
    if !output.wants_output() {
        return output.parse_complete("", &ctx);
    }
    let text = last.text();
    let parseable = last.finish_reason.unified == FinishReasonKind::Stop
        || (last.finish_reason.unified != FinishReasonKind::ToolCalls && !text.is_empty());
    if !parseable {
        return Err(Error::NoOutputGenerated);
    }
    output.parse_complete(&text, &ctx)
}

async fn run_step(
    ctx: &LoopContext,
    steps: &[StepResult],
    response_messages: &[Message],
    pending_deferred: &mut HashSet<ToolCallId>,
    cancellation: &CallCancellation,
) -> Result<(StepResult, LoopState), Error> {
    let step_started = Instant::now();
    let inputs = prepare_step_inputs(ctx, steps, response_messages, cancellation).await?;
    emit_model_call_start(ctx, &inputs).await;

    let call_ctx = ModelCallContext {
        call_id: ctx.call_id.clone(),
        step_number: inputs.step_number,
        model: inputs.identity.clone(),
        function_id: ctx.function_id().map(str::to_owned),
    };
    let model_call_started = Instant::now();
    let mut result: GenerateResult =
        retry(&ctx.config.retry_policy, cancellation.token(), |_attempt| {
            let options = inputs.options.clone();
            let model = Arc::clone(&inputs.model);
            let telemetry = &ctx.telemetry;
            let call_ctx = &call_ctx;
            let span = spans::model_call_span(&inputs.identity);
            async move {
                let outcome = telemetry
                    .execute_language_model_call(
                        call_ctx,
                        Box::pin(async move {
                            model
                                .do_generate(options)
                                .await
                                .map(|result| ModelCallOutcome::Generate(Box::new(result)))
                                .map_err(Error::from)
                        }),
                    )
                    .await?;
                match outcome {
                    ModelCallOutcome::Generate(result) => Ok(*result),
                    #[allow(unreachable_patterns, reason = "the outcome enum is non-exhaustive")]
                    _ => Err(Error::message(
                        "telemetry integration returned a stream for a generate call",
                    )),
                }
            }
            .instrument(span)
        })
        .await
        .map_err(|error| cancellation.map_error(error))?;
    let response_time = model_call_started.elapsed();
    complete_response_metadata(ctx, &inputs, &mut result.response);
    spans::log_warnings(&result.warnings, &inputs.identity);

    let step_messages: Arc<[Message]> = Arc::from(inputs.messages.clone());
    let parse_ctx = ParseContext {
        tools: &inputs.tools,
        tool_choice: inputs.tool_choice.as_ref(),
        repair: ctx.config.repair_tool_call.as_deref(),
        refine: &ctx.config.refine_tool_inputs,
        system: inputs.instructions.as_ref(),
        messages: &inputs.messages,
    };
    let (mut content, tool_calls) =
        convert_content(&result.content, &parse_ctx, &ctx.execution_tools).await;

    let mut performance = StepPerformance {
        response_time,
        effective_output_tokens_per_second: StepPerformance::tokens_per_second(
            result.usage.output.total,
            response_time,
        ),
        effective_total_tokens_per_second: StepPerformance::tokens_per_second(
            add_token_counts(result.usage.input.total, result.usage.output.total),
            response_time,
        ),
        ..StepPerformance::default()
    };
    let call_end = Arc::new(ModelCallEndEvent {
        call_id: ctx.call_id.clone(),
        step_number: inputs.step_number,
        model: inputs.identity.clone(),
        content: ctx.telemetry.record_outputs().then(|| content.clone()),
        finish_reason: result.finish_reason.clone(),
        usage: result.usage.clone(),
        response: result.response.clone(),
        performance: performance.clone(),
        warnings: result.warnings.clone(),
    });
    ctx.telemetry.on_language_model_call_end(&call_end);
    Hooks::emit(&ctx.hooks.on_language_model_call_end, call_end).await;

    super::parse_tool_call::check_tool_choice(inputs.tool_choice.as_ref(), &tool_calls)?;

    let approvals = resolve_approvals(
        ctx,
        &tool_calls,
        &step_messages,
        inputs.tools_context.as_ref(),
        cancellation,
    )
    .await;

    let mut tool_outputs: Vec<StepContent> = invalid_tool_errors(&tool_calls);
    let client_tool_calls = tool_calls
        .iter()
        .filter(|call| !call.provider_executed)
        .count();
    if is_tool_execution_allowed(&result.finish_reason) {
        let to_execute: Vec<ParsedToolCall> = tool_calls
            .iter()
            .filter(|call| {
                !call.provider_executed
                    && !call.invalid
                    && !approvals.blocked.contains(&call.tool_call_id)
            })
            .cloned()
            .collect();
        tool_outputs.extend(
            execute_tools(
                ctx,
                to_execute,
                Arc::clone(&step_messages),
                inputs.tools_context.clone(),
                cancellation,
            )
            .await?,
        );
    }
    let client_tool_outputs = tool_outputs.len();
    let result_ids: HashSet<ToolCallId> = result
        .content
        .iter()
        .filter_map(|part| match part {
            Content::ToolResult(result) => Some(result.tool_call_id.clone()),
            _ => None,
        })
        .collect();
    track_deferred(
        &tool_calls,
        &result_ids,
        &ctx.execution_tools,
        pending_deferred,
    );

    content.extend(
        approvals
            .requests
            .into_iter()
            .map(StepContent::ToolApprovalRequest),
    );
    let denied_approvals = approvals
        .responses
        .iter()
        .filter(|response| !response.approved)
        .count();
    content.extend(
        approvals
            .responses
            .into_iter()
            .map(StepContent::ToolApprovalResponse),
    );
    content.extend(tool_outputs);
    let messages = to_response_messages(&content, &ctx.config.tools);
    performance.step_time = step_started.elapsed();

    let include = ctx.config.include;
    let step = StepResult {
        step_number: inputs.step_number,
        model: inputs.identity,
        content,
        finish_reason: result.finish_reason,
        usage: result.usage,
        warnings: result.warnings,
        request: StepRequest {
            body: include
                .request_body
                .then_some(result.request.body)
                .flatten(),
            messages: include.request_messages.then_some(inputs.messages),
        },
        response: StepResponse {
            id: result.response.id,
            timestamp: result.response.timestamp,
            model_id: result.response.model_id,
            headers: result.response.headers,
            body: include
                .response_body
                .then_some(result.response.body)
                .flatten(),
            messages,
        },
        provider_metadata: result.provider_metadata,
        performance,
    };
    let state = LoopState {
        client_tool_calls,
        client_tool_outputs,
        denied_approvals,
        pending_deferred: pending_deferred.len(),
    };
    Ok((step, state))
}

/// Converts model content into step content, parsing tool calls.
pub(crate) async fn convert_content(
    content: &[Content],
    parse_ctx: &ParseContext<'_>,
    tools: &ToolSet,
) -> (Vec<StepContent>, Vec<ParsedToolCall>) {
    let mut converted: Vec<StepContent> = Vec::with_capacity(content.len());
    let mut calls: Vec<ParsedToolCall> = Vec::new();
    for part in content {
        match part {
            Content::Text {
                text,
                provider_metadata,
            } => converted.push(StepContent::Text {
                text: text.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::Reasoning {
                text,
                provider_metadata,
            } => converted.push(StepContent::Reasoning {
                text: text.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::ReasoningFile {
                data,
                media_type,
                provider_metadata,
            } => converted.push(StepContent::ReasoningFile(super::GeneratedFile {
                data: data.clone(),
                media_type: media_type.clone(),
                filename: None,
                provider_metadata: provider_metadata.clone(),
            })),
            Content::File {
                data,
                media_type,
                filename,
                provider_metadata,
            } => converted.push(StepContent::File(super::GeneratedFile {
                data: data.clone(),
                media_type: media_type.clone(),
                filename: filename.clone(),
                provider_metadata: provider_metadata.clone(),
            })),
            Content::Custom {
                kind,
                provider_metadata,
            } => converted.push(StepContent::Custom {
                kind: kind.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::Source(source) => converted.push(StepContent::Source(source.clone())),
            Content::ToolCall(call) => {
                let parsed = parse_tool_call(call, parse_ctx).await;
                calls.push(parsed.clone());
                converted.push(StepContent::ToolCall(parsed));
            }
            Content::ToolResult(result) => {
                let input = calls
                    .iter()
                    .find(|call| call.tool_call_id == result.tool_call_id)
                    .map_or(JsonValue::Null, |call| call.input.clone());
                let dynamic = result.dynamic
                    || tools
                        .get(result.tool_name.as_str())
                        .is_some_and(|tool| tool.kind().is_dynamic());
                if result.is_error {
                    converted.push(StepContent::ToolError(ToolExecutionError {
                        tool_call_id: result.tool_call_id.clone(),
                        tool_name: result.tool_name.clone(),
                        input,
                        error: ToolErrorInfo::Json {
                            value: result.result.clone(),
                        },
                        provider_executed: true,
                        dynamic,
                        provider_metadata: result.provider_metadata.clone(),
                    }));
                } else {
                    converted.push(StepContent::ToolResult(ToolResult {
                        tool_call_id: result.tool_call_id.clone(),
                        tool_name: result.tool_name.clone(),
                        input,
                        output: result.result.clone(),
                        provider_executed: true,
                        dynamic,
                        preliminary: result.preliminary,
                        execution_ms: None,
                        provider_metadata: result.provider_metadata.clone(),
                    }));
                }
            }
            Content::ToolApprovalRequest {
                approval_id,
                tool_call_id,
                provider_metadata,
            } => match calls.iter().find(|call| call.tool_call_id == *tool_call_id) {
                Some(call) => converted.push(StepContent::ToolApprovalRequest(
                    ToolApprovalRequestContent {
                        approval_id: approval_id.clone(),
                        tool_call: call.clone(),
                        reason: None,
                        is_automatic: false,
                        signature: None,
                        provider_metadata: provider_metadata.clone(),
                    },
                )),
                None => tracing::warn!(
                    target: "ferrin::generate_text",
                    %approval_id,
                    %tool_call_id,
                    "provider approval request references an unknown tool call"
                ),
            },
            #[allow(unreachable_patterns, reason = "the content enum is non-exhaustive")]
            _ => {}
        }
    }
    (converted, calls)
}
