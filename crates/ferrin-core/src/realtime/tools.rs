//! Tool support for realtime sessions: advertising the tool set to the model
//! and tracking the tool calls of a response.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use ferrin_spec::JsonValue;
use ferrin_spec::ToolName;
use ferrin_spec::realtime_model::RealtimeToolDefinition;
use ferrin_tool::DescriptionContext;
use ferrin_tool::ToolKind;
use ferrin_tool::ToolSet;

use crate::error::Error;

/// Converts a tool set into realtime tool definitions.
///
/// Function and dynamic tools are converted; provider-defined and
/// provider-executed tools are skipped because realtime sessions only
/// support function calling. Dynamic descriptions are resolved with
/// `tools_context`.
///
/// # Errors
///
/// Returns [`Error::InvalidArgument`] when `tools_context` does not match a
/// tool's context schema.
pub async fn realtime_tool_definitions(
    tools: &ToolSet,
    tools_context: Option<&JsonValue>,
) -> Result<Vec<RealtimeToolDefinition>, Error> {
    let mut definitions = Vec::with_capacity(tools.len());
    for (name, tool) in tools.iter() {
        if !matches!(tool.kind(), ToolKind::Function | ToolKind::Dynamic) {
            continue;
        }
        let tool_context = tool
            .validate_context(name, tools_context.cloned())
            .map_err(|error| Error::invalid_argument("tools_context", error.to_string()))?;
        let ctx = DescriptionContext {
            tool_context,
            #[cfg(feature = "sandbox")]
            sandbox: None,
        };
        let description = tool.resolve_description(ctx).await;
        definitions.push(RealtimeToolDefinition {
            name: name.as_str().to_owned(),
            description,
            parameters: tool.input_schema().json_schema().clone(),
        });
    }
    Ok(definitions)
}

/// Tool calls of the current tool-bearing response.
///
/// A follow-up response is requested exactly once: after the response that
/// carried the tool calls is done and every call has an output. Requesting a
/// response after each output would let the model continue without the full
/// tool context on multi-tool turns.
#[derive(Debug, Default)]
pub(super) struct ToolTurn {
    in_response: BTreeSet<String>,
    submitted: BTreeSet<String>,
    closed: bool,
    names: BTreeMap<String, ToolName>,
}

impl ToolTurn {
    /// Records a tool call announced by the model.
    pub(super) fn call_started(&mut self, call_id: &str, name: &ToolName) {
        self.in_response.insert(call_id.to_owned());
        self.names.insert(call_id.to_owned(), name.clone());
    }

    /// Returns the tool name recorded for `call_id`.
    pub(super) fn name(&self, call_id: &str) -> Option<&ToolName> {
        self.names.get(call_id)
    }

    /// Records a submitted output; returns `true` when a follow-up response
    /// should be requested now.
    pub(super) fn output_submitted(&mut self, call_id: &str) -> bool {
        self.submitted.insert(call_id.to_owned());
        self.maybe_request_response()
    }

    /// Records that the tool-bearing response finished; returns `true` when a
    /// follow-up response should be requested now.
    pub(super) fn response_done(&mut self) -> bool {
        if self.in_response.is_empty() {
            return false;
        }
        self.closed = true;
        self.maybe_request_response()
    }

    fn maybe_request_response(&mut self) -> bool {
        if !self.closed || self.in_response.is_empty() {
            return false;
        }
        if !self.in_response.is_subset(&self.submitted) {
            return false;
        }
        self.in_response.clear();
        self.submitted.clear();
        self.closed = false;
        true
    }
}
