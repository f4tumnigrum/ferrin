//! Per-step input preparation shared by the generate and stream loops:
//! `prepare_step` overrides, tool preparation and prompt conversion.

use super::PrepareStepContext;
use super::StepResult;
use super::run::LoopContext;
use crate::USER_AGENT;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::hooks::Hooks;
use crate::prompt::CallSettings;
use crate::prompt::ConvertContext;
use crate::prompt::Instructions;
use crate::prompt::PrepareToolsInput;
use crate::prompt::convert_to_prompt;
use crate::prompt::prepare_tools;
use crate::registry::resolve_language_model;
use crate::telemetry::ModelCallStartEvent;
use crate::telemetry::ModelIdentity;
use crate::telemetry::StepStartEvent;
use ferrin_message::Message;
use ferrin_spec::CallOptions;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseMetadata;
use std::sync::Arc;

/// State carried between steps of one invocation, independent of response history.
pub(crate) struct StepState {
    messages: Vec<Message>,
    instructions: Option<Instructions>,
    tools_context: Option<JsonValue>,
    runtime_context: Option<JsonValue>,
    response_count: usize,
}

impl StepState {
    pub(crate) fn new(ctx: &LoopContext) -> Self {
        Self {
            messages: ctx.initial_messages.clone(),
            instructions: ctx.instructions.clone(),
            tools_context: ctx.config.tools_context.clone(),
            runtime_context: ctx.config.runtime_context.clone(),
            response_count: 0,
        }
    }
}

/// The resolved inputs of one step after `prepare_step`.
pub(crate) struct StepInputs {
    pub(crate) step_number: u32,
    pub(crate) model: Arc<dyn DynLanguageModel>,
    pub(crate) identity: ModelIdentity,
    pub(crate) instructions: Option<Instructions>,
    pub(crate) messages: Vec<Message>,
    pub(crate) tools_context: Option<JsonValue>,
    pub(crate) runtime_context: Option<JsonValue>,
    pub(crate) tools: ferrin_tool::ToolSet,
    pub(crate) tool_choice: Option<ferrin_spec::ToolChoice>,
    pub(crate) options: CallOptions,
    pub(crate) tool_contract: Option<crate::middleware::tool_contract::ToolContract>,
    #[cfg(feature = "sandbox")]
    pub(crate) sandbox: Option<Arc<dyn ferrin_tool::Sandbox>>,
}

/// Applies `prepare_step`, prepares tools and converts the prompt.
pub(crate) async fn prepare_step_inputs(
    ctx: &LoopContext,
    steps: &[StepResult],
    response_messages: &[Message],
    state: &mut StepState,
    cancellation: &CallCancellation,
) -> Result<StepInputs, Error> {
    let step_number = u32::try_from(steps.len()).unwrap_or(u32::MAX);
    state
        .messages
        .extend(response_messages[state.response_count..].iter().cloned());
    state.response_count = response_messages.len();
    let mut messages = state.messages.clone();
    let mut instructions = state.instructions.clone();
    let mut model = Arc::clone(&ctx.model);
    let mut identity = ctx.identity.clone();
    let mut tool_choice = ctx.config.tool_choice.clone();
    let mut active_tools = ctx.config.active_tools.clone();
    let mut tool_order = ctx.config.tool_order.clone();
    let mut tools_context = state.tools_context.clone();
    let mut runtime_context = state.runtime_context.clone();
    let mut settings: CallSettings = ctx.config.settings.clone();
    #[cfg(feature = "sandbox")]
    let mut sandbox = ctx.config.sandbox.clone();

    if let Some(prepare) = &ctx.config.prepare_step {
        let overrides = prepare
            .prepare_step(PrepareStepContext {
                steps,
                step_number,
                model: &model,
                instructions: instructions.as_ref(),
                initial_instructions: ctx.instructions.as_ref(),
                messages: &messages,
                initial_messages: &ctx.initial_messages,
                response_messages,
                tools_context: tools_context.as_ref(),
                runtime_context: runtime_context.as_ref(),
                #[cfg(feature = "sandbox")]
                sandbox: ctx.config.sandbox.as_ref(),
            })
            .await?;
        if let Some(override_model) = overrides.model {
            model = resolve_language_model(&override_model)?;
            identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
        }
        if overrides.tool_choice.is_some() {
            tool_choice = overrides.tool_choice;
        }
        if overrides.active_tools.is_some() {
            active_tools = overrides.active_tools;
        }
        if let Some(order) = overrides.tool_order {
            tool_order = order;
        }
        if overrides.instructions.is_some() {
            instructions = overrides.instructions;
        }
        if let Some(override_messages) = overrides.messages {
            messages = override_messages;
        }
        if overrides.tools_context.is_some() {
            tools_context = overrides.tools_context;
        }
        if overrides.runtime_context.is_some() {
            runtime_context = overrides.runtime_context;
        }
        #[cfg(feature = "sandbox")]
        if overrides.sandbox.is_some() {
            sandbox = overrides.sandbox;
        }
        if let Some(override_settings) = overrides.settings {
            settings.merge(&override_settings);
            settings.validate()?;
        }
    }

    state.messages = messages.clone();
    state.instructions = instructions.clone();
    state.tools_context = tools_context.clone();
    state.runtime_context = runtime_context.clone();

    let step_start = Arc::new(StepStartEvent {
        runtime_context: runtime_context.clone(),
        call_id: ctx.call_id.clone(),
        step_number,
        model: identity.clone(),
        messages: ctx
            .telemetry
            .record_inputs()
            .then(|| Arc::from(messages.clone())),
    });
    ctx.telemetry.on_step_start(&step_start).await;
    Hooks::emit(&ctx.hooks.on_step_start, step_start).await;

    let tools = active_tools.as_ref().map_or_else(
        || ctx.model_tools.clone(),
        |active| ctx.model_tools.filter_active(active),
    );
    let prepared = prepare_tools(PrepareToolsInput {
        tools: &tools,
        active_tools: None,
        tool_order: &tool_order,
        tool_choice: tool_choice.clone(),
        tools_context: tools_context.as_ref(),
        #[cfg(feature = "sandbox")]
        sandbox: sandbox.clone(),
    })
    .await?;
    let supported_urls = model.supported_urls().await;
    let prompt = convert_to_prompt(
        instructions.as_ref(),
        &messages,
        ConvertContext {
            supported_urls: &supported_urls,
            download: ctx.config.download.as_deref(),
            cache: Some(&ctx.downloads),
            cancellation: cancellation.token(),
        },
    )
    .await?;

    let mut options = CallOptions::new(prompt);
    settings.apply(&mut options);
    options.tools = prepared.definitions;
    options.tool_choice = prepared.tool_choice.clone();
    options.response_format = ctx.response_format.clone();
    options.headers = std::mem::take(&mut options.headers).with_user_agent_suffix([USER_AGENT]);
    options.cancellation = cancellation.token().child_token();

    Ok(StepInputs {
        step_number,
        model,
        identity,
        instructions,
        messages,
        tools_context,
        runtime_context,
        tools,
        tool_choice: prepared.tool_choice,
        tool_contract: None,
        #[cfg(feature = "sandbox")]
        sandbox,
        options,
    })
}

/// Emits the model-call-start event.
pub(crate) async fn emit_model_call_start(ctx: &LoopContext, inputs: &StepInputs) {
    let event = Arc::new(ModelCallStartEvent {
        runtime_context: inputs.runtime_context.clone(),
        call_id: ctx.call_id.clone(),
        step_number: inputs.step_number,
        model: inputs.identity.clone(),
        call_options: ctx
            .telemetry
            .record_inputs()
            .then(|| inputs.options.to_recordable()),
    });
    ctx.telemetry.on_language_model_call_start(&event).await;
    Hooks::emit(&ctx.hooks.on_language_model_call_start, event).await;
}

/// Fills missing response id, timestamp and model id.
pub(crate) fn complete_response_metadata(
    ctx: &LoopContext,
    inputs: &StepInputs,
    response: &mut ResponseMetadata,
) {
    if response.id.is_none() {
        response.id = Some(ctx.config.id_generator.generate());
    }
    if response.timestamp.is_none() {
        response.timestamp = Some(ctx.config.clock.now());
    }
    if response.model_id.is_none() {
        response.model_id = Some(inputs.identity.model_id.clone());
    }
}

impl StepInputs {
    /// Refreshes middleware restrictions without carrying them across retries.
    pub(crate) fn refresh_tools(&mut self, original: &ferrin_tool::ToolSet) {
        if let Some(contract) = &self.tool_contract {
            (self.tools, self.tool_choice) = contract.apply(original);
        }
    }
}
