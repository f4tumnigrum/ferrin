//! Conversion of the tool set into provider tool definitions.

use ferrin_spec::JsonValue;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::ToolName;
use ferrin_tool::DescriptionContext;
use ferrin_tool::ToolSet;

use crate::error::Error;

/// Tool definitions and choice for one model call.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PreparedTools {
    /// Definitions in sending order.
    pub(crate) definitions: Vec<ToolDefinition>,
    /// Tool choice (`None` when no tools are sent).
    pub(crate) tool_choice: Option<ToolChoice>,
}

/// Inputs of [`prepare_tools`].
pub(crate) struct PrepareToolsInput<'a> {
    pub(crate) tools: &'a ToolSet,
    pub(crate) active_tools: Option<&'a [ToolName]>,
    pub(crate) tool_order: &'a [ToolName],
    pub(crate) tool_choice: Option<ToolChoice>,
    pub(crate) tools_context: Option<&'a JsonValue>,
    #[cfg(feature = "sandbox")]
    pub(crate) sandbox: Option<std::sync::Arc<dyn ferrin_tool::Sandbox>>,
}

/// Prepares the tools sent to the model.
///
/// An empty (or fully filtered) tool set yields no definitions and no tool
/// choice. Dynamic descriptions are resolved with the validated tool
/// context.
pub(crate) async fn prepare_tools(input: PrepareToolsInput<'_>) -> Result<PreparedTools, Error> {
    let filtered = match input.active_tools {
        Some(active) => input.tools.filter_active(active),
        None => input.tools.clone(),
    };
    if filtered.is_empty() {
        return Ok(PreparedTools::default());
    }
    let mut definitions = Vec::with_capacity(filtered.len());
    for (name, tool) in filtered.ordered(input.tool_order) {
        let tool_context = tool
            .validate_named_context(name, input.tools_context)
            .map_err(|error| Error::invalid_argument("tools_context", error.to_string()))?;
        let ctx = tool_context.map_or_else(
            DescriptionContext::default,
            DescriptionContext::with_tool_context,
        );
        #[cfg(feature = "sandbox")]
        let ctx = DescriptionContext {
            sandbox: input.sandbox.clone(),
            ..ctx
        };
        let description = tool.resolve_description(ctx).await;
        definitions.push(tool.definition(name.clone(), description));
    }
    Ok(PreparedTools {
        definitions,
        tool_choice: input.tool_choice,
    })
}
