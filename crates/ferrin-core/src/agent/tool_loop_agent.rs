//! The built-in tool-loop agent.

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolName;
use ferrin_tool::ToolSet;

use super::Agent;
use super::AgentCall;
use super::AgentInput;
use super::AgentStreamCall;
use crate::error::Error;
use crate::generate_text::GenerateText;
use crate::generate_text::GenerateTextResult;
use crate::generate_text::Include;
use crate::generate_text::StopCondition;
use crate::generate_text::config::CallConfig;
use crate::generate_text::step_count;
use crate::output::NoOutput;
use crate::output::Output;
use crate::output::OutputHandler;
use crate::prompt::CallSettings;
use crate::prompt::Instructions;
use crate::retry::RetryPolicy;
use crate::stream_text::StreamText;
use crate::stream_text::StreamTextResult;
use crate::telemetry::TelemetryOptions;
use crate::timeout::Timeout;

/// User-Agent product appended to agent requests.
pub const AGENT_USER_AGENT: &str = "ferrin-agent/tool-loop";

/// Default stop condition when none is configured.
const DEFAULT_MAX_STEPS: u32 = 20;

/// The effective settings of one call, as seen by [`PrepareCall`].
///
/// Every field starts with the agent's configuration (with the call's
/// input and timeout applied); the prepare function may rewrite any of
/// them. `None` means "not set".
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
#[derive(Debug)]
pub struct PrepareCallInput<Opt> {
    /// The per-call options.
    pub options: Opt,
    /// The effective settings of the call, to be returned (possibly
    /// modified).
    pub defaults: PreparedCall,
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

struct Settings<Opt, Out> {
    id: Option<String>,
    config: CallConfig,
    output: Arc<dyn OutputHandler<Out>>,
    prepare_call: Option<Arc<dyn PrepareCall<Opt>>>,
}

/// An agent that runs the tool loop of `generate_text`/`stream_text` with
/// fixed settings. Build with [`ToolLoopAgent::builder`].
pub struct ToolLoopAgent<Opt = (), Out = ()> {
    settings: Arc<Settings<Opt, Out>>,
}

impl<Opt, Out> Clone for ToolLoopAgent<Opt, Out> {
    fn clone(&self) -> Self {
        Self {
            settings: Arc::clone(&self.settings),
        }
    }
}

impl<Opt, Out> fmt::Debug for ToolLoopAgent<Opt, Out> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolLoopAgent")
            .field("id", &self.settings.id)
            .field("config", &self.settings.config)
            .field("has_prepare_call", &self.settings.prepare_call.is_some())
            .finish()
    }
}

impl ToolLoopAgent {
    /// Starts building an agent around `model`.
    #[must_use]
    pub fn builder(model: impl Into<LanguageModelRef>) -> ToolLoopAgentBuilder<(), ()> {
        ToolLoopAgentBuilder {
            id: None,
            config: CallConfig::new(model.into()),
            output: Arc::new(NoOutput),
            prepare_call: None,
            _options: PhantomData,
        }
    }
}

impl<Opt: Send + 'static, Out: Send + 'static> ToolLoopAgent<Opt, Out> {
    /// The prepared settings of a call, before the prepare function runs.
    fn effective(&self, call: &AgentCall<Opt>) -> PreparedCall {
        let config = &self.settings.config;
        PreparedCall {
            input: call.input.clone(),
            instructions: config.system.clone(),
            allow_system_in_messages: config.allow_system_in_messages,
            model: config.model.clone(),
            tools: config.tools.clone(),
            tool_choice: config.tool_choice.clone(),
            active_tools: config.active_tools.clone(),
            tool_order: config.tool_order.clone(),
            tools_context: config.tools_context.clone(),
            runtime_context: config.runtime_context.clone(),
            settings: config.settings.clone(),
            stop_conditions: if config.stop_conditions.is_empty() {
                vec![Arc::new(step_count(DEFAULT_MAX_STEPS))]
            } else {
                config.stop_conditions.clone()
            },
            timeout: call
                .timeout
                .clone()
                .unwrap_or_else(|| config.timeout.clone()),
            retry_policy: config.retry_policy.clone(),
            include: config.include,
            max_tool_concurrency: config.max_tool_concurrency,
            telemetry: config.telemetry.clone(),
        }
    }

    /// Builds the call configuration for `call`.
    async fn prepare(&self, call: AgentCall<Opt>) -> Result<CallConfig, Error> {
        let mut prepared = self.effective(&call);
        let AgentCall {
            options,
            cancellation,
            hooks,
            #[cfg(feature = "sandbox")]
            sandbox,
            ..
        } = call;
        if let Some(prepare_call) = &self.settings.prepare_call {
            prepared = prepare_call
                .prepare_call(PrepareCallInput {
                    options,
                    defaults: prepared,
                })
                .await?;
        }
        let mut config = self.settings.config.clone();
        match prepared.input {
            AgentInput::Prompt(prompt) => {
                config.prompt = Some(prompt);
                config.messages = None;
            }
            AgentInput::Messages(messages) => {
                config.prompt = None;
                config.messages = Some(messages);
            }
        }
        config.system = prepared.instructions;
        config.allow_system_in_messages = prepared.allow_system_in_messages;
        config.model = prepared.model;
        config.tools = prepared.tools;
        config.tool_choice = prepared.tool_choice;
        config.active_tools = prepared.active_tools;
        config.tool_order = prepared.tool_order;
        config.tools_context = prepared.tools_context;
        config.runtime_context = prepared.runtime_context;
        config.settings = prepared.settings;
        config.settings.headers = config
            .settings
            .headers
            .with_user_agent_suffix([AGENT_USER_AGENT]);
        config.stop_conditions = prepared.stop_conditions;
        config.timeout = prepared.timeout;
        config.retry_policy = prepared.retry_policy;
        config.include = prepared.include;
        config.max_tool_concurrency = prepared.max_tool_concurrency;
        config.telemetry = prepared.telemetry;
        if config.telemetry.function_id.is_none() {
            config.telemetry.function_id = self.settings.id.clone();
        }
        config.cancellation = cancellation;
        // Agent hooks run before the call's hooks.
        config.hooks = self.settings.config.hooks.clone().merged(hooks);
        #[cfg(feature = "sandbox")]
        if let Some(sandbox) = sandbox {
            config.sandbox = Some(sandbox);
        }
        Ok(config)
    }
}

impl<Opt: Send + 'static, Out: Send + 'static> Agent for ToolLoopAgent<Opt, Out> {
    type Options = Opt;
    type Output = Out;

    fn id(&self) -> Option<&str> {
        self.settings.id.as_deref()
    }

    fn tools(&self) -> &ToolSet {
        &self.settings.config.tools
    }

    async fn generate(&self, call: AgentCall<Opt>) -> Result<GenerateTextResult<Out>, Error> {
        let config = self.prepare(call).await?;
        GenerateText {
            config,
            output: Arc::clone(&self.settings.output),
        }
        .await
    }

    async fn stream(&self, call: AgentStreamCall<Opt>) -> Result<StreamTextResult<Out>, Error> {
        let AgentStreamCall { call, stream } = call;
        let config = self.prepare(call).await?;
        StreamText {
            config,
            output: Arc::clone(&self.settings.output),
            stream,
        }
        .await
    }
}

/// Builder for [`ToolLoopAgent`]. Besides the methods below it accepts every
/// call setting of `generate_text` (prompt-independent ones).
pub struct ToolLoopAgentBuilder<Opt, Out> {
    id: Option<String>,
    pub(crate) config: CallConfig,
    output: Arc<dyn OutputHandler<Out>>,
    prepare_call: Option<Arc<dyn PrepareCall<Opt>>>,
    _options: PhantomData<fn() -> Opt>,
}

impl<Opt, Out> fmt::Debug for ToolLoopAgentBuilder<Opt, Out> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolLoopAgentBuilder")
            .field("id", &self.id)
            .field("config", &self.config)
            .field("has_prepare_call", &self.prepare_call.is_some())
            .finish()
    }
}

crate::builder::impl_call_builder!(ToolLoopAgentBuilder<Opt, Out>);

impl<Opt, Out> ToolLoopAgentBuilder<Opt, Out> {
    /// Sets the agent id (also the default telemetry `function_id`).
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the system instructions (alias of `system`).
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<Instructions>) -> Self {
        self.config.system = Some(instructions.into());
        self
    }

    /// Sets the structured output.
    #[must_use]
    pub fn output<T>(self, output: Output<T>) -> ToolLoopAgentBuilder<Opt, T> {
        ToolLoopAgentBuilder {
            id: self.id,
            config: self.config,
            output: output.handler(),
            prepare_call: self.prepare_call,
            _options: PhantomData,
        }
    }

    /// Sets the per-call options type. Resets any prepare function set
    /// before (it was typed for the previous options).
    #[must_use]
    pub fn call_options<O>(self) -> ToolLoopAgentBuilder<O, Out> {
        ToolLoopAgentBuilder {
            id: self.id,
            config: self.config,
            output: self.output,
            prepare_call: None,
            _options: PhantomData,
        }
    }

    /// Sets the function that rewrites settings per call.
    #[must_use]
    pub fn prepare_call(mut self, prepare: impl PrepareCall<Opt> + 'static) -> Self {
        self.prepare_call = Some(Arc::new(prepare));
        self
    }

    /// Finishes the agent.
    #[must_use]
    pub fn build(self) -> ToolLoopAgent<Opt, Out> {
        ToolLoopAgent {
            settings: Arc::new(Settings {
                id: self.id,
                config: self.config,
                output: self.output,
                prepare_call: self.prepare_call,
            }),
        }
    }
}
