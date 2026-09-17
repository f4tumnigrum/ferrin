//! The built-in tool-loop agent.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use ferrin_spec::LanguageModelRef;
use ferrin_tool::ToolSet;

use super::Agent;
use super::AgentCall;
use super::AgentInput;
use super::AgentStreamCall;
use super::PrepareCall;
use super::PrepareCallInput;
use super::PreparedCall;
use super::options::CallOptionsValidator;
use crate::error::Error;
use crate::generate_text::GenerateText;
use crate::generate_text::GenerateTextResult;
use crate::generate_text::config::CallConfig;
use crate::generate_text::step_count;
use crate::output::NoOutput;
use crate::output::Output;
use crate::output::OutputHandler;
use crate::prompt::Instructions;
use crate::stream_text::StreamText;
use crate::stream_text::StreamTextResult;

/// User-Agent product appended to agent requests.
pub const AGENT_USER_AGENT: &str = "ferrin-agent/tool-loop";

/// Default stop condition when none is configured.
const DEFAULT_MAX_STEPS: u32 = 20;

struct Settings<Opt, Out> {
    id: Option<String>,
    config: CallConfig,
    output: Arc<dyn OutputHandler<Out>>,
    prepare_call: Option<Arc<dyn PrepareCall<Opt>>>,
    call_options_validator: Option<CallOptionsValidator<Opt>>,
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
            .field(
                "has_call_options_schema",
                &self.settings.call_options_validator.is_some(),
            )
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
            call_options_validator: None,
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
            tool_approval: config.tool_approval.clone(),
            tool_approval_secret: config.tool_approval_secret.clone(),
            tool_callers: config.tool_callers.clone(),
            prepare_step: config.prepare_step.clone(),
            repair_tool_call: config.repair_tool_call.clone(),
            refine_tool_inputs: config.refine_tool_inputs.clone(),
            download: config.download.clone(),
            settings: config.settings.clone(),
            stop_conditions: if config.stop_conditions.is_empty() {
                vec![Arc::new(step_count(DEFAULT_MAX_STEPS))]
            } else {
                config.stop_conditions.clone()
            },
            timeout: config.timeout.clone(),
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
            timeout,
            hooks,
            #[cfg(feature = "sandbox")]
            sandbox,
            ..
        } = call;
        let options = match &self.settings.call_options_validator {
            Some(validate) => validate(options)?,
            None => options,
        };
        if let Some(prepare_call) = &self.settings.prepare_call {
            prepared = prepare_call
                .prepare_call(PrepareCallInput {
                    options,
                    defaults: prepared,
                    #[cfg(feature = "sandbox")]
                    sandbox: sandbox.clone(),
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
        config.tool_approval = prepared.tool_approval;
        config.tool_approval_secret = prepared.tool_approval_secret;
        config.tool_callers = prepared.tool_callers;
        config.prepare_step = prepared.prepare_step;
        config.repair_tool_call = prepared.repair_tool_call;
        config.refine_tool_inputs = prepared.refine_tool_inputs;
        config.download = prepared.download;
        config.settings = prepared.settings;
        config.settings.headers = config
            .settings
            .headers
            .with_user_agent_suffix([AGENT_USER_AGENT]);
        config.stop_conditions = prepared.stop_conditions;
        config.timeout = timeout.unwrap_or(prepared.timeout);
        config.retry_policy = prepared.retry_policy;
        config.include = prepared.include;
        config.max_tool_concurrency = prepared.max_tool_concurrency;
        config.telemetry = prepared.telemetry;
        if config.telemetry.function_id.is_none() {
            config.telemetry.function_id = self.settings.id.clone();
        }
        config.cancellation = cancellation;
        // Agent hooks begin before call hooks; all run concurrently to completion.
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
    call_options_validator: Option<CallOptionsValidator<Opt>>,
    _options: PhantomData<fn() -> Opt>,
}

impl<Opt, Out> fmt::Debug for ToolLoopAgentBuilder<Opt, Out> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolLoopAgentBuilder")
            .field("id", &self.id)
            .field("config", &self.config)
            .field("has_prepare_call", &self.prepare_call.is_some())
            .field(
                "has_call_options_schema",
                &self.call_options_validator.is_some(),
            )
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
            call_options_validator: self.call_options_validator,
            _options: PhantomData,
        }
    }

    /// Sets the per-call options type, resetting preparation and validation
    /// callbacks configured for the previous type.
    #[must_use]
    pub fn call_options<O>(self) -> ToolLoopAgentBuilder<O, Out> {
        ToolLoopAgentBuilder {
            id: self.id,
            config: self.config,
            output: self.output,
            prepare_call: None,
            call_options_validator: None,
            _options: PhantomData,
        }
    }

    /// Sets the function that rewrites settings per call.
    #[must_use]
    pub fn prepare_call(mut self, prepare: impl PrepareCall<Opt> + 'static) -> Self {
        self.prepare_call = Some(Arc::new(prepare));
        self
    }

    /// Validates and normalizes call options before preparing a generation or stream.
    ///
    /// The schema receives serialized options and its validated value replaces them.
    /// Validation failures return [`Error::InvalidArgument`] before callbacks or model
    /// calls start. Without this option, call options need not implement `Serialize`.
    /// Changing the option type with [`Self::call_options`] clears this schema.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferrin_core::ToolLoopAgent;
    /// use ferrin_schema::Schema;
    /// use serde_json::{Value, json};
    ///
    /// let agent = ToolLoopAgent::builder("provider:model")
    ///     .call_options::<Value>()
    ///     .call_options_schema(Schema::from_json_schema(json!({
    ///         "type": "object", "required": ["tenant"],
    ///         "properties": { "tenant": { "type": "string" } }
    ///     })))
    ///     .build();
    /// ```
    #[must_use]
    pub fn call_options_schema(mut self, schema: ferrin_schema::Schema<Opt>) -> Self
    where
        Opt: serde::Serialize + 'static,
    {
        self.call_options_validator = Some(super::options::validator(schema));
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
                call_options_validator: self.call_options_validator,
            }),
        }
    }
}
