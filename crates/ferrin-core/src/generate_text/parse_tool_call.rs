//! Parsing, validation and repair of model tool calls.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;
use ferrin_tool::ToolSet;

use super::ParsedToolCall;
use crate::error::BoxError;
use crate::error::Error;
use crate::prompt::Instructions;

/// Information handed to a [`ToolCallRepair`].
#[derive(Debug)]
pub struct RepairRequest<'a> {
    /// The tool call as issued by the model.
    pub tool_call: &'a ToolCall,
    /// The tool set of the call.
    pub tools: &'a ToolSet,
    /// System instructions of the call.
    pub system: Option<&'a Instructions>,
    /// Messages sent to the model in this step.
    pub messages: &'a [Message],
    /// The error that triggered the repair ([`Error::NoSuchTool`] or
    /// [`Error::InvalidToolInput`]).
    pub error: &'a Error,
}

impl RepairRequest<'_> {
    /// JSON schema of `tool_name`'s input, when the tool exists.
    #[must_use]
    pub fn input_schema(&self, tool_name: &str) -> Option<&JsonValue> {
        self.tools
            .get(tool_name)
            .map(|tool| tool.input_schema().json_schema())
    }
}

/// Repairs tool calls the model got wrong (unknown tool or invalid input).
///
/// Return `Ok(None)` to give up (the original error is kept), `Ok(Some(call))`
/// to re-parse the repaired call, or `Err` to fail with
/// [`Error::ToolCallRepair`].
pub trait ToolCallRepair: Send + Sync {
    /// Attempts a repair.
    fn repair<'a>(
        &'a self,
        request: RepairRequest<'a>,
    ) -> BoxFuture<'a, Result<Option<ToolCall>, BoxError>>;
}

/// Rewrites a validated tool input before execution.
pub type RefineToolInputFn =
    Arc<dyn Fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, Error>> + Send + Sync>;

/// Refinement functions by tool name.
#[derive(Clone, Default)]
pub struct RefineToolInputs(HashMap<ToolName, RefineToolInputFn>);

impl RefineToolInputs {
    /// Registers `refine` for `tool_name`.
    pub fn insert(&mut self, tool_name: impl Into<ToolName>, refine: RefineToolInputFn) {
        self.0.insert(tool_name.into(), refine);
    }

    /// Looks up the function for `tool_name`.
    #[must_use]
    pub fn get(&self, tool_name: &str) -> Option<&RefineToolInputFn> {
        self.0.get(tool_name)
    }

    /// Returns `true` when no function is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for RefineToolInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

/// Inputs of [`parse_tool_call`].
pub(crate) struct ParseContext<'a> {
    pub(crate) tools: &'a ToolSet,
    pub(crate) tool_choice: Option<&'a ToolChoice>,
    pub(crate) repair: Option<&'a dyn ToolCallRepair>,
    pub(crate) refine: &'a RefineToolInputs,
    pub(crate) system: Option<&'a Instructions>,
    pub(crate) messages: &'a [Message],
}

/// Parses a model tool call. Failures never propagate: the call is marked
/// `invalid` (and `dynamic`) with the error message attached.
pub(crate) async fn parse_tool_call(call: &ToolCall, ctx: &ParseContext<'_>) -> ParsedToolCall {
    match try_parse(call, ctx).await {
        Ok(parsed) => parsed,
        Err(error) => invalid_call(call, &error, ctx.tools),
    }
}

/// Parses a model tool call, returning the error on failure.
pub(crate) async fn try_parse(
    call: &ToolCall,
    ctx: &ParseContext<'_>,
) -> Result<ParsedToolCall, Error> {
    if ctx.tools.is_empty() {
        if call.provider_executed && call.dynamic {
            let input = parse_raw_input(call)?;
            return Ok(ParsedToolCall {
                tool_call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                input,
                provider_executed: true,
                dynamic: true,
                invalid: false,
                error: None,
                title: None,
                tool_metadata: None,
                provider_metadata: call.provider_metadata.clone(),
            });
        }
        return Err(Error::no_such_tool(call.tool_name.clone(), Vec::new()));
    }
    let error = match do_parse(call, ctx).await {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error,
    };
    let Some(repair) = ctx.repair else {
        return Err(error);
    };
    if !matches!(error, Error::NoSuchTool { .. } | Error::InvalidToolInput(_)) {
        return Err(error);
    }
    let repaired = repair
        .repair(RepairRequest {
            tool_call: call,
            tools: ctx.tools,
            system: ctx.system,
            messages: ctx.messages,
            error: &error,
        })
        .await;
    match repaired {
        Err(cause) => Err(Error::ToolCallRepair {
            original: Box::new(error),
            cause,
        }),
        Ok(None) => Err(error),
        Ok(Some(repaired_call)) => do_parse(&repaired_call, ctx).await,
    }
}

async fn do_parse(call: &ToolCall, ctx: &ParseContext<'_>) -> Result<ParsedToolCall, Error> {
    let Some(tool) = ctx.tools.get(call.tool_name.as_str()) else {
        return Err(Error::no_such_tool(
            call.tool_name.clone(),
            ctx.tools.names().cloned().collect(),
        ));
    };
    if let Some(ToolChoice::Tool { tool_name }) = ctx.tool_choice
        && *tool_name != call.tool_name
    {
        return Err(Error::ToolChoiceViolation {
            expected: tool_name.clone(),
            actual: call.tool_name.clone(),
        });
    }
    let raw = parse_raw_input(call)?;
    let mut input = if call.provider_executed || tool.kind().is_provider_executed() {
        raw
    } else {
        tool.validate_input(&call.tool_name, raw).map_err(|error| {
            Error::invalid_tool_input(call.tool_name.clone(), call.input.clone(), Box::new(error))
        })?
    };
    if let Some(refine) = ctx.refine.get(call.tool_name.as_str()) {
        input = refine(input).await?;
    }
    Ok(ParsedToolCall {
        tool_call_id: call.tool_call_id.clone(),
        tool_name: call.tool_name.clone(),
        input,
        provider_executed: call.provider_executed,
        dynamic: call.dynamic || tool.kind().is_dynamic(),
        invalid: false,
        error: None,
        title: tool.title().map(str::to_owned),
        tool_metadata: tool.metadata().cloned(),
        provider_metadata: call.provider_metadata.clone(),
    })
}

fn parse_raw_input(call: &ToolCall) -> Result<JsonValue, Error> {
    if call.input.trim().is_empty() {
        return Ok(JsonValue::Object(serde_json::Map::new()));
    }
    ferrin_schema::json::parse(&call.input).map_err(|error| {
        Error::invalid_tool_input(call.tool_name.clone(), call.input.clone(), Box::new(error))
    })
}

fn invalid_call(call: &ToolCall, error: &Error, tools: &ToolSet) -> ParsedToolCall {
    let input = serde_json::from_str::<JsonValue>(&call.input)
        .unwrap_or_else(|_| JsonValue::String(call.input.clone()));
    ParsedToolCall {
        tool_call_id: call.tool_call_id.clone(),
        tool_name: call.tool_name.clone(),
        input,
        provider_executed: call.provider_executed,
        dynamic: true,
        invalid: true,
        error: Some(error.to_string()),
        title: None,
        tool_metadata: tools
            .get(call.tool_name.as_str())
            .and_then(|tool| tool.metadata().cloned()),
        provider_metadata: call.provider_metadata.clone(),
    }
}

/// Checks the effective choice shared by both generation loops.
pub(crate) fn check_tool_choice(
    choice: Option<&ToolChoice>,
    calls: &[ParsedToolCall],
) -> Result<(), Error> {
    match choice {
        Some(ToolChoice::Required) if calls.is_empty() => {
            Err(Error::ToolChoiceNotSatisfied { expected: None })
        }
        Some(ToolChoice::Tool { tool_name })
            if !calls.iter().any(|call| call.tool_name == *tool_name) =>
        {
            Err(Error::ToolChoiceNotSatisfied {
                expected: Some(tool_name.clone()),
            })
        }
        _ => Ok(()),
    }
}
