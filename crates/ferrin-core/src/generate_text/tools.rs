//! Tool execution inside the generation loop: approval resolution, invalid
//! call reporting, deferred result tracking and the concurrent execution of
//! client tools. Shared by the generate and stream loops.

use super::ApprovalContext;
use super::ApprovalStatus;
use super::ParsedToolCall;
use super::StepContent;
use super::ToolApprovalRequestContent;
use super::ToolApprovalResponseContent;
use super::ToolErrorInfo;
use super::ToolExecutionError;
use super::ToolResult;
use super::approval::resolve_approval;
use super::approval::signature;
use super::execute_tool::execute_tool;
use super::run::LoopContext;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::hooks::Hooks;
use crate::telemetry::TelemetryDispatcher;
use crate::telemetry::ToolExecutionContext;
use crate::telemetry::ToolExecutionEndEvent;
use crate::telemetry::ToolExecutionStartEvent;
use crate::telemetry::ToolOutcome;
use crate::telemetry::spans;
use ferrin_message::Message;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::Instrument;

/// Tool errors for invalid client tool calls.
pub(crate) fn invalid_tool_errors(calls: &[ParsedToolCall]) -> Vec<StepContent> {
    calls
        .iter()
        .filter(|call| call.invalid && call.dynamic && !call.provider_executed)
        .map(|call| {
            StepContent::ToolError(ToolExecutionError {
                tool_call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                input: call.input.clone(),
                error: ToolErrorInfo::text(call.error.clone().unwrap_or_default()),
                provider_executed: false,
                dynamic: true,
                provider_metadata: None,
            })
        })
        .collect()
}

/// Records provider tool calls with deferred results and clears the ones
/// whose results (`result_ids`) arrived.
pub(crate) fn track_deferred(
    calls: &[ParsedToolCall],
    result_ids: &HashSet<ToolCallId>,
    tools: &ToolSet,
    pending: &mut HashSet<ToolCallId>,
) {
    for call in calls.iter().filter(|call| call.provider_executed) {
        if supports_deferred_results(tools, call.tool_name.as_str())
            && !result_ids.contains(&call.tool_call_id)
        {
            pending.insert(call.tool_call_id.clone());
        }
    }
    for id in result_ids {
        pending.remove(id);
    }
}

/// Outcome of approval resolution for one step.
#[derive(Debug, Default)]
pub(crate) struct StepApprovals {
    pub(crate) requests: Vec<ToolApprovalRequestContent>,
    pub(crate) responses: Vec<ToolApprovalResponseContent>,
    pub(crate) blocked: HashSet<ToolCallId>,
}

/// Approval outcome of one tool call that needs a decision.
#[derive(Debug)]
pub(crate) struct CallApproval {
    /// The request recorded in the step content.
    pub(crate) request: ToolApprovalRequestContent,
    /// The automatic decision, when the policy decided.
    pub(crate) response: Option<ToolApprovalResponseContent>,
    /// Whether execution is blocked (denied or awaiting the user).
    pub(crate) blocked: bool,
}

/// Resolves the approval status of one tool call; `None` for invalid calls
/// and calls that need no approval.
pub(crate) async fn resolve_call_approval(
    ctx: &LoopContext,
    call: &ParsedToolCall,
    messages: &Arc<[Message]>,
    tools_context: Option<&JsonValue>,
    cancellation: &CallCancellation,
) -> Result<Option<CallApproval>, Error> {
    if call.invalid {
        return Ok(None);
    }
    let approval_ctx = ApprovalContext {
        messages,
        tools_context,
    };
    let tool = ctx.execution_tools.get(call.tool_name.as_str());
    let tool_context = match tool {
        Some(tool) => ctx.tool_context(
            tool,
            &call.tool_call_id,
            &call.tool_name,
            messages,
            tools_context,
            cancellation,
        )?,
        None => ToolContext::new(call.tool_call_id.clone()),
    };
    let status = resolve_approval(
        call,
        tool.map(AsRef::as_ref),
        ctx.config.tool_approval.as_deref(),
        approval_ctx,
        || tool_context,
    )
    .await;
    if matches!(status, ApprovalStatus::NotApplicable) {
        return Ok(None);
    }
    let approval_id = ferrin_spec::ApprovalId::new(ctx.config.id_generator.generate());
    let signature = ctx.config.tool_approval_secret.as_ref().map(|secret| {
        signature::sign(
            secret,
            signature::SignedFields {
                approval_id: &approval_id,
                tool_call_id: &call.tool_call_id,
                tool_name: &call.tool_name,
                input: &call.input,
            },
        )
    });
    let reason = status.reason().map(str::to_owned);
    let is_automatic = !matches!(status, ApprovalStatus::UserApproval { .. });
    let request = ToolApprovalRequestContent {
        approval_id: approval_id.clone(),
        tool_call: call.clone(),
        reason: reason.clone(),
        is_automatic,
        signature,
        provider_metadata: None,
    };
    let (response, blocked) = match status {
        ApprovalStatus::Approved { .. } => (
            Some(ToolApprovalResponseContent {
                approval_id,
                tool_call: call.clone(),
                approved: true,
                reason,
                provider_executed: call.provider_executed,
            }),
            false,
        ),
        ApprovalStatus::Denied { .. } => (
            Some(ToolApprovalResponseContent {
                approval_id,
                tool_call: call.clone(),
                approved: false,
                reason,
                provider_executed: call.provider_executed,
            }),
            true,
        ),
        _ => (None, true),
    };
    Ok(Some(CallApproval {
        request,
        response,
        blocked,
    }))
}

/// Resolves the approval status of every valid tool call.
pub(crate) async fn resolve_approvals(
    ctx: &LoopContext,
    calls: &[ParsedToolCall],
    messages: &Arc<[Message]>,
    tools_context: Option<&JsonValue>,
    cancellation: &CallCancellation,
) -> Result<StepApprovals, Error> {
    let mut approvals = StepApprovals::default();
    for call in calls {
        let Some(approval) =
            resolve_call_approval(ctx, call, messages, tools_context, cancellation).await?
        else {
            continue;
        };
        if approval.blocked {
            approvals.blocked.insert(call.tool_call_id.clone());
        }
        approvals.requests.push(approval.request);
        approvals.responses.extend(approval.response);
    }
    Ok(approvals)
}

/// Per-task plumbing of one tool execution.
pub(crate) struct ToolTask {
    pub(crate) telemetry: TelemetryDispatcher,
    pub(crate) hooks: Arc<Hooks>,
    pub(crate) call_id: String,
    pub(crate) timeout: Option<Duration>,
    pub(crate) tool_context: ToolContext,
}

/// Returns `true` when `tool_name` is a provider-executed tool whose results
/// may arrive in a later step.
pub(crate) fn supports_deferred_results(tools: &ToolSet, tool_name: &str) -> bool {
    tools.get(tool_name).is_some_and(|tool| {
        matches!(
            tool.kind(),
            ferrin_tool::ToolKind::ProviderExecuted {
                supports_deferred_results: true,
                ..
            }
        )
    })
}

/// Executes `calls` concurrently (bounded by `max_tool_concurrency`) and
/// returns their outputs in call order. Cancellation of a tool aborts the
/// whole call.
pub(crate) async fn execute_tools(
    ctx: &LoopContext,
    calls: Vec<ParsedToolCall>,
    messages: Arc<[Message]>,
    tools_context: Option<JsonValue>,
    cancellation: &CallCancellation,
) -> Result<Vec<StepContent>, Error> {
    let mut results: Vec<Option<StepContent>> = (0..calls.len()).map(|_| None).collect();
    let mut pending = calls.into_iter().enumerate().filter_map(|(index, call)| {
        let tool = ctx.execution_tools.get(call.tool_name.as_str())?;
        tool.is_executable()
            .then(|| (index, call, Arc::clone(tool)))
    });
    let max = ctx.config.max_tool_concurrency.unwrap_or(usize::MAX);
    let mut tasks: JoinSet<(usize, Result<StepContent, Error>)> = JoinSet::new();
    let mut spawn_next =
        |tasks: &mut JoinSet<(usize, Result<StepContent, Error>)>| -> Result<bool, Error> {
            let Some((index, call, tool)) = pending.next() else {
                return Ok(false);
            };
            let task = ctx.tool_task(
                &tool,
                &call,
                &messages,
                tools_context.as_ref(),
                cancellation,
            )?;
            let span = spans::tool_span(call.tool_name.as_str(), call.tool_call_id.as_str());
            tasks.spawn(
                async move {
                    let result = run_tool_call(call, tool, task, None).await;
                    (index, result)
                }
                .instrument(span),
            );
            Ok(true)
        };
    for _ in 0..max {
        if !spawn_next(&mut tasks)? {
            break;
        }
    }
    while let Some(joined) = tasks.join_next().await {
        let (index, result) =
            joined.map_err(|error| Error::message(format!("tool task failed: {error}")))?;
        match result {
            Ok(content) => results[index] = Some(content),
            Err(error) => {
                tasks.abort_all();
                return Err(cancellation.map_error(error));
            }
        }
        spawn_next(&mut tasks)?;
    }
    Ok(results.into_iter().flatten().collect())
}

/// Executes one tool call: emits start/end events, wraps the execution in
/// the telemetry integrations and converts the outcome into step content.
///
/// Preliminary results are forwarded to `progress` when given. Cancellation
/// of the tool is fatal for the call and returned as [`Error::Cancelled`].
pub(crate) async fn run_tool_call(
    call: ParsedToolCall,
    tool: Arc<Tool>,
    task: ToolTask,
    progress: Option<tokio::sync::mpsc::Sender<StepContent>>,
) -> Result<StepContent, Error> {
    let record_inputs = task.telemetry.record_inputs();
    let record_outputs = task.telemetry.record_outputs();
    let start = Arc::new(ToolExecutionStartEvent {
        call_id: task.call_id.clone(),
        tool_call_id: call.tool_call_id.clone(),
        tool_name: call.tool_name.clone(),
        input: record_inputs.then(|| call.input.clone()),
    });
    task.telemetry.on_tool_execution_start(&start);
    Hooks::emit(&task.hooks.on_tool_execution_start, start).await;

    let exec_ctx = ToolExecutionContext {
        call_id: task.call_id.clone(),
        tool_call_id: call.tool_call_id.clone(),
        tool_name: call.tool_name.clone(),
        input: record_inputs.then(|| call.input.clone()),
    };
    let execution = execute_tool(&tool, call.input.clone(), task.tool_context, task.timeout);
    let preliminary_template = ToolResult {
        tool_call_id: call.tool_call_id.clone(),
        tool_name: call.tool_name.clone(),
        input: call.input.clone(),
        output: JsonValue::Null,
        provider_executed: false,
        dynamic: call.dynamic,
        preliminary: true,
        execution_ms: None,
        provider_metadata: None,
    };
    let started = Instant::now();
    let outcome = task
        .telemetry
        .execute_tool(
            &exec_ctx,
            Box::pin(async move {
                let mut execution = std::pin::pin!(execution);
                use futures_util::StreamExt as _;
                while let Some(event) = execution.next().await {
                    match event {
                        super::execute_tool::ToolExecutionEvent::Preliminary(value) => {
                            if let Some(progress) = &progress {
                                let content = StepContent::ToolResult(ToolResult {
                                    output: value,
                                    ..preliminary_template.clone()
                                });
                                if progress.send(content).await.is_err() {
                                    return Err(ToolError::Cancelled);
                                }
                            }
                        }
                        super::execute_tool::ToolExecutionEvent::Finished { output } => {
                            return output.map(|output| ToolOutcome { output });
                        }
                    }
                }
                Err(ToolError::message("tool execution ended unexpectedly"))
            }),
        )
        .await;
    let duration = started.elapsed();
    let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    tracing::Span::current().record("ferrin.tool.duration_ms", duration_ms);

    let (content, recorded_output, error) = match outcome {
        Ok(ToolOutcome { output }) => (
            Ok(StepContent::ToolResult(ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                input: call.input.clone(),
                output: output.clone(),
                provider_executed: false,
                dynamic: call.dynamic,
                preliminary: false,
                execution_ms: Some(duration_ms),
                provider_metadata: None,
            })),
            record_outputs.then_some(ToolOutcome { output }),
            None,
        ),
        Err(ToolError::Cancelled) => (
            Err(Error::Cancelled),
            None,
            Some(ToolErrorInfo::text("tool execution cancelled")),
        ),
        Err(tool_error) => {
            let info = ToolErrorInfo::from(&tool_error);
            (
                Ok(StepContent::ToolError(ToolExecutionError {
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    input: call.input.clone(),
                    error: info.clone(),
                    provider_executed: false,
                    dynamic: call.dynamic,
                    provider_metadata: None,
                })),
                None,
                Some(info),
            )
        }
    };
    let end = Arc::new(ToolExecutionEndEvent {
        call_id: task.call_id,
        tool_call_id: call.tool_call_id,
        tool_name: call.tool_name,
        output: recorded_output,
        error,
        duration,
    });
    task.telemetry.on_tool_execution_end(&end);
    Hooks::emit(&task.hooks.on_tool_execution_end, end).await;
    content
}
