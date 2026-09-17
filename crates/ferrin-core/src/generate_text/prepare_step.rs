//! Per-step overrides.

use std::fmt;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_spec::BoxFuture;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;

use super::StepResult;
use crate::error::Error;
use crate::prompt::CallSettings;
use crate::prompt::Instructions;

/// Information available to a [`PrepareStep`] callback.
pub struct PrepareStepContext<'a> {
    /// Steps completed so far.
    pub steps: &'a [StepResult],
    /// Zero-based index of the step about to run.
    pub step_number: u32,
    /// The model configured for the call.
    pub model: &'a Arc<dyn DynLanguageModel>,
    /// Instructions retained from the preceding step, initially configured on the call.
    pub instructions: Option<&'a Instructions>,
    /// Instructions originally configured for this invocation.
    pub initial_instructions: Option<&'a Instructions>,
    /// Current messages, including new responses since the latest message override.
    pub messages: &'a [Message],
    /// The initial messages of the call.
    pub initial_messages: &'a [Message],
    /// Response messages accumulated so far.
    pub response_messages: &'a [Message],
    /// The tools context retained from the preceding step.
    pub tools_context: Option<&'a JsonValue>,
    /// Application state retained from the preceding step.
    pub runtime_context: Option<&'a JsonValue>,
    /// The sandbox configured for the invocation before this step's override.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<&'a Arc<dyn ferrin_tool::Sandbox>>,
}

impl fmt::Debug for PrepareStepContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrepareStepContext")
            .field("step_number", &self.step_number)
            .field("provider", self.model.provider())
            .field("model_id", self.model.model_id())
            .field("steps", &self.steps.len())
            .field("messages", &self.messages.len())
            .finish_non_exhaustive()
    }
}

/// Overrides returned by a [`PrepareStep`] callback. Unset fields keep the
/// prior state for messages, instructions and contexts; other fields keep call defaults.
#[derive(Default)]
pub struct StepOverrides {
    /// Model for this step.
    pub model: Option<LanguageModelRef>,
    /// Tool choice for this step.
    pub tool_choice: Option<ToolChoice>,
    /// Active tools for this step.
    pub active_tools: Option<Vec<ToolName>>,
    /// Tool order for this step.
    pub tool_order: Option<Vec<ToolName>>,
    /// Instructions for this and subsequent steps.
    pub instructions: Option<Instructions>,
    /// Messages for this and subsequent steps (new responses are appended).
    pub messages: Option<Vec<Message>>,
    /// Tools context for this and subsequent steps.
    pub tools_context: Option<JsonValue>,
    /// Application state for this and subsequent steps (`None` preserves the prior value).
    pub runtime_context: Option<JsonValue>,
    /// Sandbox for this step only; an unset value uses the invocation sandbox.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<dyn ferrin_tool::Sandbox>>,
    /// Sampling settings overlaid on the call settings.
    pub settings: Option<CallSettings>,
}

impl fmt::Debug for StepOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepOverrides")
            .field("model", &self.model)
            .field("tool_choice", &self.tool_choice)
            .field("active_tools", &self.active_tools)
            .field("tool_order", &self.tool_order)
            .field("instructions", &self.instructions)
            .field("messages", &self.messages)
            .field("has_tools_context", &self.tools_context.is_some())
            .field("has_runtime_context", &self.runtime_context.is_some())
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl StepOverrides {
    /// No overrides.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Overrides the model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<LanguageModelRef>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Overrides the tool choice.
    #[must_use]
    pub fn with_tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Overrides the active tools.
    #[must_use]
    pub fn with_active_tools(
        mut self,
        names: impl IntoIterator<Item = impl Into<ToolName>>,
    ) -> Self {
        self.active_tools = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Overrides the tool order.
    #[must_use]
    pub fn with_tool_order(mut self, names: impl IntoIterator<Item = impl Into<ToolName>>) -> Self {
        self.tool_order = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Overrides the instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<Instructions>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Overrides the messages.
    #[must_use]
    pub fn with_messages(mut self, messages: impl IntoIterator<Item = Message>) -> Self {
        self.messages = Some(messages.into_iter().collect());
        self
    }

    /// Overrides the tools context.
    #[must_use]
    pub fn with_tools_context(mut self, context: JsonValue) -> Self {
        self.tools_context = Some(context);
        self
    }

    /// Replaces application state for this and subsequent steps.
    #[must_use]
    pub fn with_runtime_context(mut self, context: JsonValue) -> Self {
        self.runtime_context = Some(context);
        self
    }

    /// Uses a sandbox for this step only.
    #[cfg(feature = "sandbox")]
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: Arc<dyn ferrin_tool::Sandbox>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Overlays sampling settings.
    #[must_use]
    pub fn with_settings(mut self, settings: CallSettings) -> Self {
        self.settings = Some(settings);
        self
    }
}

/// Computes overrides before each step.
///
/// Implemented for every `Fn(&PrepareStepContext<'_>) -> StepOverrides`
/// closure; implement the trait directly when the decision is asynchronous.
pub trait PrepareStep: Send + Sync {
    /// Returns the overrides for the step described by `ctx`.
    fn prepare_step<'a>(
        &'a self,
        ctx: PrepareStepContext<'a>,
    ) -> BoxFuture<'a, Result<StepOverrides, Error>>;
}

impl<F> PrepareStep for F
where
    F: Fn(&PrepareStepContext<'_>) -> StepOverrides + Send + Sync,
{
    fn prepare_step<'a>(
        &'a self,
        ctx: PrepareStepContext<'a>,
    ) -> BoxFuture<'a, Result<StepOverrides, Error>> {
        let overrides = self(&ctx);
        Box::pin(async move { Ok(overrides) })
    }
}
