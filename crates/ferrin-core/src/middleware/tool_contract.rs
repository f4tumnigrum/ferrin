//! Per-attempt tool restrictions shared by middleware and the execution loop.

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_spec::CallOptions;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;
use ferrin_tool::ToolSet;

tokio::task_local! {
    static CURRENT: ToolContract;
}

#[derive(Clone)]
pub(crate) struct ToolContract(Arc<Mutex<Settings>>);

struct Settings {
    names: Vec<ToolName>,
    choice: Option<ToolChoice>,
}

impl ToolContract {
    /// Starts a fresh scope for every request attempt.
    pub(crate) fn new(options: &CallOptions) -> Self {
        Self(Arc::new(Mutex::new(Settings {
            names: options
                .tools
                .iter()
                .map(|tool| tool.name().clone())
                .collect(),
            choice: options.tool_choice.clone(),
        })))
    }

    /// Runs a model attempt in its own scope.
    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    /// Captures the scope before a middleware hands its continuation away.
    pub(crate) fn current() -> Option<Self> {
        CURRENT.try_with(Clone::clone).ok()
    }

    /// A deeper layer cannot re-enable a tool removed by an outer layer.
    pub(crate) fn observe(&self, options: &CallOptions) {
        let mut settings = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        settings
            .names
            .retain(|name| options.tools.iter().any(|tool| tool.name() == name));
        settings.choice = options.tool_choice.clone();
    }

    /// Restores the captured scope even when the continuation changes tasks.
    pub(crate) async fn continue_call<F: Future>(contract: Option<Self>, future: F) -> F::Output {
        match contract {
            Some(contract) => CURRENT.scope(contract, future).await,
            None => future.await,
        }
    }

    /// Applies the latest restriction before execution, including lazy streams.
    pub(crate) fn apply(&self, original: &ToolSet) -> (ToolSet, Option<ToolChoice>) {
        let settings = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let choice = match &settings.choice {
            Some(ToolChoice::Tool { tool_name }) if !settings.names.contains(tool_name) => None,
            Some(ToolChoice::Required) if settings.names.is_empty() => None,
            choice => choice.clone(),
        };
        (original.filter_active(&settings.names), choice)
    }
}
