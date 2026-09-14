//! Caller restrictions: which callers (the model directly, or other tools)
//! may trigger a tool, and how caller tools receive their callees.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ferrin_spec::ProviderOptions;
use ferrin_spec::ToolName;
use ferrin_spec::error::InvalidArgumentError;

use crate::set::ToolSet;
use crate::tool::Tool;

/// Something that may trigger a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ToolCaller {
    /// The model calls the tool directly.
    Direct,
    /// Another tool (one with a [`ToolCallerDefinition`]) calls it.
    Tool(ToolName),
}

/// Allowed callers per tool. Tools not listed keep the default (direct
/// calls only).
pub type ToolCallers = HashMap<ToolName, Vec<ToolCaller>>;

/// Binds callees to a caller tool.
pub type LocalBindFn = Arc<dyn Fn(ToolSet) -> Tool + Send + Sync>;
/// Adjusts a callee's provider options so the provider routes its calls
/// through the caller tool.
pub type PrepareProviderOptionsFn =
    Arc<dyn Fn(Option<ProviderOptions>) -> ProviderOptions + Send + Sync>;

/// How a caller tool reaches its callees.
#[derive(Clone)]
#[non_exhaustive]
pub enum ToolCallerDefinition {
    /// The caller runs locally and receives the callees as a [`ToolSet`].
    Local(LocalBindFn),
    /// The provider performs the calls; callees are marked through provider
    /// options.
    Provider(PrepareProviderOptionsFn),
}

impl fmt::Debug for ToolCallerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(_) => f.write_str("Local(..)"),
            Self::Provider(_) => f.write_str("Provider(..)"),
        }
    }
}

impl ToolCallerDefinition {
    /// A local caller.
    pub fn local(bind: impl Fn(ToolSet) -> Tool + Send + Sync + 'static) -> Self {
        Self::Local(Arc::new(bind))
    }

    /// A provider caller.
    pub fn provider(
        prepare: impl Fn(Option<ProviderOptions>) -> ProviderOptions + Send + Sync + 'static,
    ) -> Self {
        Self::Provider(Arc::new(prepare))
    }
}

/// Checks a caller configuration against the tool set.
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] (argument `tool_callers`) when a listed
/// tool is unknown or a caller is not a tool with a caller definition.
pub fn validate_tool_callers(
    tools: &ToolSet,
    callers: &ToolCallers,
) -> Result<(), InvalidArgumentError> {
    for (tool_name, list) in callers {
        if !tools.contains(tool_name.as_str()) {
            return Err(InvalidArgumentError::new(
                "tool_callers",
                format!("unknown tool \"{tool_name}\"."),
            ));
        }
        for caller in list {
            if let ToolCaller::Tool(caller_name) = caller
                && tools
                    .get(caller_name.as_str())
                    .is_none_or(|tool| tool.caller_definition().is_none())
            {
                return Err(InvalidArgumentError::new(
                    "tool_callers",
                    format!("tool \"{tool_name}\" contains an invalid caller."),
                ));
            }
        }
    }
    Ok(())
}

/// Tool sets derived from a caller configuration.
#[derive(Debug, Clone)]
pub struct PreparedToolCallers {
    /// Tools the core may execute (all tools, with callers bound).
    pub execution_tools: ToolSet,
    /// Tools sent to the model (callees reachable only through local callers
    /// are removed).
    pub model_tools: ToolSet,
}

/// Applies a caller configuration: callees of provider callers get their
/// provider options prepared, callees of local callers are bound into the
/// caller and hidden from the model, and tools without a direct or provider
/// caller are removed from the model tool set.
#[must_use]
pub fn prepare_tools_for_callers(tools: &ToolSet, callers: &ToolCallers) -> PreparedToolCallers {
    let mut execution = tools.clone();
    let mut model = tools.clone();
    let mut local_by_caller: HashMap<ToolName, ToolSet> = HashMap::new();

    for (tool_name, tool) in tools {
        let Some(list) = callers.get(tool_name) else {
            continue;
        };
        let mut direct = false;
        let mut via_provider = false;
        let mut prepared: Tool = (**tool).clone();
        for caller in list {
            match caller {
                ToolCaller::Direct => direct = true,
                ToolCaller::Tool(caller_name) => {
                    let Some(definition) = execution
                        .get(caller_name.as_str())
                        .and_then(|caller_tool| caller_tool.caller_definition().cloned())
                    else {
                        continue;
                    };
                    match definition {
                        ToolCallerDefinition::Provider(prepare) => {
                            via_provider = true;
                            let options = prepare(prepared.provider_options.take());
                            prepared = prepared.with_provider_options(Some(options));
                        }
                        ToolCallerDefinition::Local(_) => {
                            let entry = local_by_caller.entry(caller_name.clone()).or_default();
                            entry.replace(tool_name.clone(), Arc::new(prepared.clone()));
                        }
                        #[allow(
                            unreachable_patterns,
                            reason = "ToolCallerDefinition is non-exhaustive"
                        )]
                        _ => {}
                    }
                }
                #[allow(unreachable_patterns, reason = "ToolCaller is non-exhaustive")]
                _ => {}
            }
        }
        let prepared = Arc::new(prepared);
        execution.replace(tool_name.clone(), Arc::clone(&prepared));
        if direct || via_provider {
            model.replace(tool_name.clone(), prepared);
        } else {
            model.remove(tool_name.as_str());
        }
    }

    let snapshot: Vec<(ToolName, Arc<Tool>)> = execution
        .iter()
        .map(|(name, tool)| (name.clone(), Arc::clone(tool)))
        .collect();
    for (caller_name, caller_tool) in snapshot {
        let Some(ToolCallerDefinition::Local(bind)) = caller_tool.caller_definition() else {
            continue;
        };
        let callees = local_by_caller.remove(&caller_name).unwrap_or_default();
        let bound = Arc::new(bind(callees));
        execution.replace(caller_name.clone(), Arc::clone(&bound));
        if model.contains(caller_name.as_str()) {
            model.replace(caller_name, bound);
        }
    }

    PreparedToolCallers {
        execution_tools: execution,
        model_tools: model,
    }
}
