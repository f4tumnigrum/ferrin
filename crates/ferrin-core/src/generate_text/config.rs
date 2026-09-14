//! Configuration shared by `generate_text` and `stream_text`.

use std::fmt;
use std::sync::Arc;

use ferrin_message::Message;
use ferrin_provider_util::IdGenerator;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;
use ferrin_tool::ToolCallers;
use ferrin_tool::ToolSet;
use secrecy::SecretBox;
use tokio_util::sync::CancellationToken;

use super::ApprovalPolicy;
use super::PrepareStep;
use super::RefineToolInputs;
use super::StopCondition;
use super::ToolCallRepair;
use crate::clock::Clock;
use crate::clock::default_clock;
use crate::hooks::Hooks;
use crate::ids::default_id_generator;
use crate::prompt::CallSettings;
use crate::prompt::DownloadFn;
use crate::prompt::Instructions;
use crate::retry::RetryPolicy;
use crate::telemetry::TelemetryOptions;
use crate::timeout::Timeout;

/// Which large payloads are copied into step results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Include {
    /// Keep the raw request body.
    pub request_body: bool,
    /// Keep the messages sent to the model.
    pub request_messages: bool,
    /// Keep the raw response body.
    pub response_body: bool,
}

impl Default for Include {
    fn default() -> Self {
        Self::none()
    }
}

impl Include {
    /// Keeps nothing.
    #[must_use]
    pub fn none() -> Self {
        Self {
            request_body: false,
            request_messages: false,
            response_body: false,
        }
    }

    /// Keeps everything.
    #[must_use]
    pub fn all() -> Self {
        Self {
            request_body: true,
            request_messages: true,
            response_body: true,
        }
    }
}

/// Everything a text generation call needs, independent of streaming.
#[derive(Clone)]
pub(crate) struct CallConfig {
    pub(crate) model: LanguageModelRef,
    pub(crate) system: Option<Instructions>,
    pub(crate) prompt: Option<String>,
    pub(crate) messages: Option<Vec<Message>>,
    pub(crate) allow_system_in_messages: bool,
    pub(crate) settings: CallSettings,
    pub(crate) tools: ToolSet,
    pub(crate) tool_choice: Option<ToolChoice>,
    pub(crate) active_tools: Option<Vec<ToolName>>,
    pub(crate) tool_order: Vec<ToolName>,
    pub(crate) tools_context: Option<JsonValue>,
    pub(crate) tool_approval: Option<Arc<dyn ApprovalPolicy>>,
    pub(crate) tool_approval_secret: Option<Arc<SecretBox<[u8]>>>,
    pub(crate) tool_callers: ToolCallers,
    pub(crate) repair_tool_call: Option<Arc<dyn ToolCallRepair>>,
    pub(crate) refine_tool_inputs: RefineToolInputs,
    #[cfg(feature = "sandbox")]
    pub(crate) sandbox: Option<Arc<dyn ferrin_tool::Sandbox>>,
    pub(crate) stop_conditions: Vec<Arc<dyn StopCondition>>,
    pub(crate) prepare_step: Option<Arc<dyn PrepareStep>>,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) timeout: Timeout,
    pub(crate) cancellation: CancellationToken,
    pub(crate) download: Option<Arc<dyn DownloadFn>>,
    pub(crate) include: Include,
    pub(crate) max_tool_concurrency: Option<usize>,
    pub(crate) telemetry: TelemetryOptions,
    pub(crate) hooks: Hooks,
    pub(crate) id_generator: Arc<dyn IdGenerator>,
    pub(crate) clock: Arc<dyn Clock>,
}

impl CallConfig {
    pub(crate) fn new(model: LanguageModelRef) -> Self {
        Self {
            model,
            system: None,
            prompt: None,
            messages: None,
            allow_system_in_messages: false,
            settings: CallSettings::default(),
            tools: ToolSet::new(),
            tool_choice: None,
            active_tools: None,
            tool_order: Vec::new(),
            tools_context: None,
            tool_approval: None,
            tool_approval_secret: None,
            tool_callers: ToolCallers::new(),
            repair_tool_call: None,
            refine_tool_inputs: RefineToolInputs::default(),
            #[cfg(feature = "sandbox")]
            sandbox: None,
            stop_conditions: Vec::new(),
            prepare_step: None,
            retry_policy: RetryPolicy::default(),
            timeout: Timeout::none(),
            cancellation: CancellationToken::new(),
            download: None,
            include: Include::default(),
            max_tool_concurrency: None,
            telemetry: TelemetryOptions::default(),
            hooks: Hooks::default(),
            id_generator: default_id_generator(),
            clock: default_clock(),
        }
    }
}

impl fmt::Debug for CallConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallConfig")
            .field("model", &self.model)
            .field("system", &self.system)
            .field("prompt", &self.prompt)
            .field("messages", &self.messages.as_ref().map(Vec::len))
            .field("allow_system_in_messages", &self.allow_system_in_messages)
            .field("settings", &self.settings)
            .field("tools", &self.tools.names().collect::<Vec<_>>())
            .field("tool_choice", &self.tool_choice)
            .field("active_tools", &self.active_tools)
            .field("tool_order", &self.tool_order)
            .field("tools_context", &self.tools_context)
            .field("tool_approval", &self.tool_approval.is_some())
            .field("tool_approval_secret", &self.tool_approval_secret.is_some())
            .field("tool_callers", &self.tool_callers)
            .field("repair_tool_call", &self.repair_tool_call.is_some())
            .field("refine_tool_inputs", &self.refine_tool_inputs)
            .field("stop_conditions", &self.stop_conditions.len())
            .field("prepare_step", &self.prepare_step.is_some())
            .field("retry_policy", &self.retry_policy)
            .field("timeout", &self.timeout)
            .field("download", &self.download.is_some())
            .field("include", &self.include)
            .field("max_tool_concurrency", &self.max_tool_concurrency)
            .field("telemetry", &self.telemetry)
            .field("hooks", &self.hooks)
            .finish_non_exhaustive()
    }
}
