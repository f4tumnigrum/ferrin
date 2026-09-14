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

/// The resolved inputs of one step after `prepare_step`.
pub(crate) struct StepInputs {
    pub(crate) step_number: u32,
    pub(crate) model: Arc<dyn DynLanguageModel>,
    pub(crate) identity: ModelIdentity,
    pub(crate) instructions: Option<Instructions>,
    pub(crate) messages: Vec<Message>,
    pub(crate) tools_context: Option<JsonValue>,
    pub(crate) tool_choice: Option<ferrin_spec::ToolChoice>,
    pub(crate) options: CallOptions,
}

/// Applies `prepare_step`, prepares tools and converts the prompt.
pub(crate) async fn prepare_step_inputs(
    ctx: &LoopContext,
    steps: &[StepResult],
    response_messages: &[Message],
    cancellation: &CallCancellation,
) -> Result<StepInputs, Error> {
    let step_number = u32::try_from(steps.len()).unwrap_or(u32::MAX);
    let mut messages: Vec<Message> = ctx
        .initial_messages
        .iter()
        .chain(response_messages.iter())
        .cloned()
        .collect();
    let mut instructions = ctx.instructions.clone();
    let mut model = Arc::clone(&ctx.model);
    let mut identity = ctx.identity.clone();
    let mut tool_choice = ctx.config.tool_choice.clone();
    let mut active_tools = ctx.config.active_tools.clone();
    let mut tool_order = ctx.config.tool_order.clone();
    let mut tools_context = ctx.config.tools_context.clone();
    let mut settings: CallSettings = ctx.config.settings.clone();

    if let Some(prepare) = &ctx.config.prepare_step {
        let overrides = prepare
            .prepare_step(PrepareStepContext {
                steps,
                step_number,
                model: &identity,
                instructions: instructions.as_ref(),
                messages: &messages,
                initial_messages: &ctx.initial_messages,
                response_messages,
                tools_context: tools_context.as_ref(),
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
        if let Some(override_settings) = overrides.settings {
            settings.merge(&override_settings);
            settings.validate()?;
        }
    }

    let step_start = Arc::new(StepStartEvent {
        call_id: ctx.call_id.clone(),
        step_number,
        model: identity.clone(),
        messages: ctx
            .telemetry
            .record_inputs()
            .then(|| Arc::from(messages.clone())),
    });
    ctx.telemetry.on_step_start(&step_start);
    Hooks::emit(&ctx.hooks.on_step_start, step_start).await;

    let prepared = prepare_tools(PrepareToolsInput {
        tools: &ctx.model_tools,
        active_tools: active_tools.as_deref(),
        tool_order: &tool_order,
        tool_choice: tool_choice.clone(),
        tools_context: tools_context.as_ref(),
        #[cfg(feature = "sandbox")]
        sandbox: ctx.config.sandbox.clone(),
    })
    .await?;
    let supported_urls = model.supported_urls().await;
    let prompt = convert_to_prompt(
        instructions.as_ref(),
        &messages,
        ConvertContext {
            supported_urls: &supported_urls,
            download: ctx.config.download.as_deref(),
            cancellation: cancellation.token(),
        },
    )
    .await?;

    let mut options = CallOptions::new(prompt);
    settings.apply(&mut options);
    options.tools = prepared.definitions;
    options.tool_choice = prepared.tool_choice;
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
        tool_choice,
        options,
    })
}

/// Emits the model-call-start event.
pub(crate) async fn emit_model_call_start(ctx: &LoopContext, inputs: &StepInputs) {
    let event = Arc::new(ModelCallStartEvent {
        call_id: ctx.call_id.clone(),
        step_number: inputs.step_number,
        model: inputs.identity.clone(),
        call_options: ctx
            .telemetry
            .record_inputs()
            .then(|| inputs.options.to_recordable()),
    });
    ctx.telemetry.on_language_model_call_start(&event);
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
