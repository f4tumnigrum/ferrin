//! Per-call agent configuration and preparation callbacks.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;
use ferrin_tool::ToolCallers;
use ferrin_tool::ToolSet;
use secrecy::SecretBox;

use super::AgentInput;
use crate::error::Error;
use crate::generate_text::ApprovalPolicy;
use crate::generate_text::Include;
use crate::generate_text::PrepareStep;
use crate::generate_text::RefineToolInputs;
use crate::generate_text::StopCondition;
use crate::generate_text::ToolCallRepair;
use crate::prompt::CallSettings;
use crate::prompt::DownloadFn;
use crate::prompt::Instructions;
use crate::retry::RetryPolicy;
use crate::telemetry::TelemetryOptions;
use crate::timeout::Timeout;

/// The effective settings of one call, as seen by [`PrepareCall`].
///
/// Fields start with the agent's configuration and the call's input; the
/// prepare function may rewrite them. `None` means "not set". An explicit
/// call timeout takes precedence after preparation.
#[derive(Clone)]
pub struct PreparedCall {
    /// The prompt or conversation.
    pub input: AgentInput,
    /// System instructions.
    pub instructions: Option<Instructions>,
    /// Whether system messages are allowed inside the conversation.
    pub allow_system_in_messages: bool,
    /// The model.
    pub model: LanguageModelRef,
    /// The tools.
    pub tools: ToolSet,
    /// Tool choice.
    pub tool_choice: Option<ToolChoice>,
    /// Tools sent to the model (all when `None`).
    pub active_tools: Option<Vec<ToolName>>,
    /// Tool order sent to the model.
    pub tool_order: Vec<ToolName>,
    /// Shared tool context.
    pub tools_context: Option<JsonValue>,
    /// Application state for the generation lifecycle, separate from tool context.
    pub runtime_context: Option<JsonValue>,
    /// Call-level approval policy; `None` uses the tools' own approval declarations.
    pub tool_approval: Option<Arc<dyn ApprovalPolicy>>,
    /// Secret used to sign approval requests, if configured.
    pub tool_approval_secret: Option<Arc<SecretBox<[u8]>>>,
    /// Allowed callers for individual tools.
    pub tool_callers: ToolCallers,
    /// Callback that selects settings before each step.
    pub prepare_step: Option<Arc<dyn PrepareStep>>,
    /// Callback that repairs invalid tool calls.
    pub repair_tool_call: Option<Arc<dyn ToolCallRepair>>,
    /// Input normalization functions indexed by tool name.
    pub refine_tool_inputs: RefineToolInputs,
    /// Downloader for file URLs that the model cannot fetch itself.
    pub download: Option<Arc<dyn DownloadFn>>,
    /// Sampling settings, headers and provider options.
    pub settings: CallSettings,
    /// Stop conditions (default: twenty steps).
    pub stop_conditions: Vec<Arc<dyn StopCondition>>,
    /// Timeouts.
    pub timeout: Timeout,
    /// Retry policy.
    pub retry_policy: RetryPolicy,
    /// Payloads copied into step results.
    pub include: Include,
    /// Tool concurrency limit.
    pub max_tool_concurrency: Option<usize>,
    /// Telemetry options.
    pub telemetry: TelemetryOptions,
}

impl fmt::Debug for PreparedCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedCall")
            .field("input", &self.input)
            .field("instructions", &self.instructions)
            .field("allow_system_in_messages", &self.allow_system_in_messages)
            .field("model", &self.model)
            .field("tools", &self.tools.names().collect::<Vec<_>>())
            .field("tool_choice", &self.tool_choice)
            .field("active_tools", &self.active_tools)
            .field("tool_order", &self.tool_order)
            .field("tools_context", &self.tools_context)
            .field("runtime_context", &self.runtime_context.is_some())
            .field("tool_approval", &self.tool_approval.is_some())
            .field("tool_approval_secret", &self.tool_approval_secret.is_some())
            .field("tool_callers", &self.tool_callers)
            .field("prepare_step", &self.prepare_step.is_some())
            .field("repair_tool_call", &self.repair_tool_call.is_some())
            .field("refine_tool_inputs", &self.refine_tool_inputs)
            .field("download", &self.download.is_some())
            .field("settings", &self.settings)
            .field("stop_conditions", &self.stop_conditions.len())
            .field("timeout", &self.timeout)
            .field("retry_policy", &self.retry_policy)
            .field("include", &self.include)
            .field("max_tool_concurrency", &self.max_tool_concurrency)
            .finish_non_exhaustive()
    }
}

/// Input of [`PrepareCall`].
pub struct PrepareCallInput<Opt> {
    /// The per-call options.
    pub options: Opt,
    /// The effective settings of the call, to be returned (possibly
    /// modified).
    pub defaults: PreparedCall,
    /// The sandbox supplied on this invocation, visible during preparation.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<dyn ferrin_tool::Sandbox>>,
}

impl<Opt: fmt::Debug> fmt::Debug for PrepareCallInput<Opt> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("PrepareCallInput");
        debug
            .field("options", &self.options)
            .field("defaults", &self.defaults);
        #[cfg(feature = "sandbox")]
        debug.field("has_sandbox", &self.sandbox.is_some());
        debug.finish()
    }
}

/// Rewrites the settings of a call from its options (templated
/// instructions, per-tenant tools, ...).
pub trait PrepareCall<Opt>: Send + Sync {
    /// Produces the settings to use.
    fn prepare_call(
        &self,
        input: PrepareCallInput<Opt>,
    ) -> BoxFuture<'_, Result<PreparedCall, Error>>;
}

impl<Opt, F, Fut> PrepareCall<Opt> for F
where
    F: Fn(PrepareCallInput<Opt>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<PreparedCall, Error>> + Send + 'static,
{
    fn prepare_call(
        &self,
        input: PrepareCallInput<Opt>,
    ) -> BoxFuture<'_, Result<PreparedCall, Error>> {
        Box::pin(self(input))
    }
}
