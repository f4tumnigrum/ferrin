//! Assembly of the Responses API request body from call options.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::ResponseFormat;
use serde_json::json;

use super::api_types::ResponsesRequest;
use super::convert_prompt::ConversionContext;
use super::convert_prompt::convert_prompt_for_provider;
use super::convert_tools::convert_tools;
use super::convert_tools::tool_name_mapping;
use super::options::ResponsesProviderOptions;
use crate::capabilities::ModelCapabilities;
use crate::config::OpenAiConfig;
use crate::json_schema::normalize_json_schema;

/// A prepared request.
#[derive(Debug)]
pub struct PreparedRequest {
    /// Request body.
    pub body: ResponsesRequest,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// Tool name mapping used for the response.
    pub tool_name_mapping: ToolNameMapping,
    /// Custom name of the declared web search tool.
    pub web_search_tool_name: Option<String>,
    /// Whether responses are stored (drives reasoning part completion).
    pub store: bool,
}

/// Parses the call-level provider options, falling back to the `openai` key
/// when the configured key is different and absent.
pub(crate) fn call_options(
    config: &OpenAiConfig,
    options: &CallOptions,
) -> Result<ResponsesProviderOptions, ProviderError> {
    let parsed = parse_provider_options::<ResponsesProviderOptions>(
        &config.provider_options_key,
        &options.provider_options,
    )?;
    if parsed.is_none() && config.provider_options_key != "openai" {
        return Ok(parse_provider_options("openai", &options.provider_options)?.unwrap_or_default());
    }
    Ok(parsed.unwrap_or_default())
}

fn add_include(include: &mut Option<Vec<String>>, key: &str) {
    let list = include.get_or_insert_with(Vec::new);
    if !list.iter().any(|value| value == key) {
        list.push(key.to_owned());
    }
}

/// Builds the request body.
///
/// # Errors
///
/// Returns conversion errors (invalid options, unsupported parts).
pub fn prepare_request(
    config: &OpenAiConfig,
    model_id: &str,
    options: &CallOptions,
) -> Result<PreparedRequest, ProviderError> {
    let mut warnings = Vec::new();
    let caps = ModelCapabilities::for_model(model_id);
    let openai = call_options(config, options)?;

    for (value, feature) in [
        (options.top_k.map(f64::from), "topK"),
        (options.seed.map(|seed| seed as f64), "seed"),
        (options.presence_penalty, "presencePenalty"),
        (options.frequency_penalty, "frequencyPenalty"),
    ] {
        if value.is_some() {
            warnings.push(Warning::unsupported(feature));
        }
    }
    if options
        .stop_sequences
        .as_ref()
        .is_some_and(|s| !s.is_empty())
    {
        warnings.push(Warning::unsupported("stopSequences"));
    }

    let mut reasoning_effort: Option<String> = openai.reasoning_effort.clone().or_else(|| {
        is_custom_reasoning(options.reasoning).then(|| options.reasoning.as_str().to_owned())
    });
    if let Some(effort) = &reasoning_effort
        && let Some(supported) = caps.supported_reasoning_efforts
        && !supported.contains(&effort.as_str())
    {
        warnings.push(Warning::unsupported_with_details(
            "reasoningEffort",
            format!(
                "{model_id} only supports the following reasoning efforts: {}",
                supported.join(", ")
            ),
        ));
        reasoning_effort = None;
    }
    let reasoning_summary = match &openai.reasoning_summary {
        Some(summary) => Some(summary.clone()),
        None => reasoning_effort
            .as_deref()
            .filter(|effort| *effort != "none")
            .map(|_| "detailed".to_owned()),
    };
    let is_reasoning_model = openai.force_reasoning.unwrap_or(caps.is_reasoning_model);
    if openai.conversation.is_some() && openai.previous_response_id.is_some() {
        warnings.push(Warning::unsupported_with_details(
            "conversation",
            "conversation and previousResponseId cannot be used together",
        ));
    }

    let mapping = tool_name_mapping(&options.tools);
    let strict_json_schema = openai.strict_json_schema.unwrap_or(true);
    let tools = convert_tools(
        &options.tools,
        options.tool_choice.as_ref(),
        &mapping,
        strict_json_schema,
        &config.provider_options_key,
    )?;
    warnings.extend(tools.warnings.iter().cloned());

    let store = openai.store.unwrap_or(true);
    let ctx = ConversionContext {
        system_message_mode: openai.system_message_mode.unwrap_or(if is_reasoning_model {
            crate::capabilities::SystemMessageMode::Developer
        } else {
            caps.system_message_mode
        }),
        store,
        has_conversation: openai.conversation.is_some(),
        has_previous_response_id: openai.previous_response_id.is_some(),
        tool_name_mapping: &mapping,
        provider_tools: &tools.provider_tools,
        file_id_prefixes: &config.file_id_prefixes,
        explicit_message_item_type: config.explicit_message_item_type,
        provider_options_key: &config.provider_options_key,
        pass_through_unsupported_files: openai.pass_through_unsupported_files.unwrap_or(false),
    };
    let converted = convert_prompt_for_provider(&options.prompt, &ctx, &config.name)?;
    warnings.extend(converted.warnings);
    let mut input = converted.input;

    if let Some(update) = &openai.reasoning_effort_update {
        let supported = caps.supports_configuration_update
            && openai.reasoning_mode.as_deref() != Some("pro")
            && openai.context_management.is_none()
            && openai.truncation.as_deref() != Some("auto");
        if supported {
            input.insert(
                0,
                json!({"type": "configuration_update", "reasoning": {"effort": update}}),
            );
        } else {
            warnings.push(Warning::unsupported_with_details(
                "reasoningEffortUpdate",
                if caps.supports_configuration_update {
                    "reasoningEffortUpdate requires standard reasoning mode without automatic compaction or automatic truncation"
                } else {
                    "reasoningEffortUpdate is only supported by GPT-6 and later models"
                },
            ));
        }
    }
    if openai.compaction_trigger == Some(true) {
        input.push(json!({"type": "compaction_trigger"}));
    }

    let mut text: Option<JsonObject> = None;
    if let Some(ResponseFormat::Json {
        schema,
        name,
        description,
    }) = &options.response_format
    {
        let format = match schema {
            Some(schema) => {
                let (normalized, schema_warnings) = normalize_json_schema(schema)?;
                warnings.extend(schema_warnings);
                let mut format = json!({
                    "type": "json_schema",
                    "strict": strict_json_schema,
                    "name": name.clone().unwrap_or_else(|| "response".to_owned()),
                    "schema": normalized,
                });
                if let Some(description) = description
                    && let Some(object) = format.as_object_mut()
                {
                    object.insert(
                        "description".to_owned(),
                        JsonValue::from(description.as_str()),
                    );
                }
                format
            }
            None => json!({"type": "json_object"}),
        };
        text.get_or_insert_with(JsonObject::new)
            .insert("format".to_owned(), format);
    }
    if let Some(verbosity) = &openai.text_verbosity {
        text.get_or_insert_with(JsonObject::new)
            .insert("verbosity".to_owned(), JsonValue::from(verbosity.as_str()));
    }

    let mut include = openai.include.clone();
    let mut top_logprobs = openai
        .logprobs
        .and_then(super::options::LogprobsOption::top_logprobs);
    if top_logprobs.is_some() {
        add_include(&mut include, "message.output_text.logprobs");
    }
    if tools.has_web_search
        && config.supports_web_search_sources_include
        && openai.include_web_search_sources != Some(false)
    {
        add_include(&mut include, "web_search_call.action.sources");
    }
    if tools.has_code_interpreter {
        add_include(&mut include, "code_interpreter_call.outputs");
    }
    if openai.store == Some(false) && is_reasoning_model {
        add_include(&mut include, "reasoning.encrypted_content");
    }

    let mut reasoning: Option<JsonObject> = None;
    if is_reasoning_model {
        let mut object = JsonObject::new();
        if let Some(effort) = &reasoning_effort {
            object.insert("effort".to_owned(), JsonValue::from(effort.as_str()));
        }
        if let Some(summary) = &reasoning_summary {
            object.insert("summary".to_owned(), JsonValue::from(summary.as_str()));
        }
        if let Some(mode) = &openai.reasoning_mode {
            object.insert("mode".to_owned(), JsonValue::from(mode.as_str()));
        }
        if let Some(context) = &openai.reasoning_context {
            object.insert("context".to_owned(), JsonValue::from(context.as_str()));
        }
        if !object.is_empty() {
            reasoning = Some(object);
        }
    }

    let mut temperature = options.temperature;
    let mut top_p = options.top_p;
    let mut service_tier = openai.service_tier.clone();
    let mut prompt_cache_retention = openai.prompt_cache_retention.clone();
    if caps.supports_configuration_update && prompt_cache_retention.is_some() {
        prompt_cache_retention = None;
        warnings.push(Warning::unsupported_with_details(
            "promptCacheRetention",
            "promptCacheRetention is not supported by GPT-6 and later models; use promptCacheOptions instead",
        ));
    }
    if is_reasoning_model {
        let sampling_allowed =
            reasoning_effort.as_deref() == Some("none") && caps.supports_non_reasoning_parameters;
        if !sampling_allowed {
            if temperature.take().is_some() {
                warnings.push(Warning::unsupported_with_details(
                    "temperature",
                    "temperature is not supported for reasoning models",
                ));
            }
            if top_p.take().is_some() {
                warnings.push(Warning::unsupported_with_details(
                    "topP",
                    "topP is not supported for reasoning models",
                ));
            }
            let has_logprobs_include = include
                .as_ref()
                .is_some_and(|list| list.iter().any(|v| v == "message.output_text.logprobs"));
            if caps.supported_reasoning_efforts.is_some()
                && (top_logprobs.is_some() || has_logprobs_include)
            {
                top_logprobs = None;
                if let Some(list) = include.as_mut() {
                    list.retain(|v| v != "message.output_text.logprobs");
                    if list.is_empty() {
                        include = None;
                    }
                }
                warnings.push(Warning::unsupported_with_details(
                    "logprobs",
                    "logprobs is not supported for reasoning models",
                ));
            }
        }
    } else {
        for (value, feature) in [
            (openai.reasoning_effort.as_ref(), "reasoningEffort"),
            (openai.reasoning_summary.as_ref(), "reasoningSummary"),
            (openai.reasoning_mode.as_ref(), "reasoningMode"),
            (openai.reasoning_context.as_ref(), "reasoningContext"),
        ] {
            if value.is_some() {
                warnings.push(Warning::unsupported_with_details(
                    feature,
                    format!("{feature} is not supported for non-reasoning models"),
                ));
            }
        }
    }
    if service_tier.as_deref() == Some("flex") && !caps.supports_flex_processing {
        warnings.push(Warning::unsupported_with_details(
            "serviceTier",
            "flex processing is only available for o3, o4-mini, and gpt-5 models",
        ));
        service_tier = None;
    }
    if matches!(service_tier.as_deref(), Some("priority" | "fast"))
        && !caps.supports_priority_processing
    {
        warnings.push(Warning::unsupported_with_details(
            "serviceTier",
            "priority processing is only available for supported models (gpt-4, gpt-5, gpt-5-mini, o3, o4-mini) and requires Enterprise access. gpt-5-nano is not supported",
        ));
        service_tier = None;
    }

    let body = ResponsesRequest {
        model: model_id.to_owned(),
        input,
        temperature,
        top_p,
        max_output_tokens: options.max_output_tokens,
        text,
        conversation: openai.conversation.clone(),
        max_tool_calls: openai.max_tool_calls,
        metadata: openai.metadata.clone(),
        parallel_tool_calls: openai.parallel_tool_calls,
        previous_response_id: openai.previous_response_id.clone(),
        store: openai.store,
        user: openai.user.clone(),
        instructions: openai.instructions.clone(),
        service_tier,
        include,
        prompt_cache_key: openai.prompt_cache_key.clone(),
        prompt_cache_options: openai.prompt_cache_options.as_ref().map(|options| {
            let mut object = JsonObject::new();
            if let Some(retention) = &options.retention {
                object.insert("retention".to_owned(), JsonValue::from(retention.as_str()));
            }
            object
        }),
        prompt_cache_retention,
        safety_identifier: openai.safety_identifier.clone(),
        top_logprobs,
        truncation: openai.truncation.clone(),
        context_management: openai.context_management.as_ref().map(|list| {
            list.iter()
                .map(|cm| {
                    let mut object = JsonObject::new();
                    object.insert("type".to_owned(), JsonValue::from(cm.kind.as_str()));
                    if let Some(threshold) = cm.compact_threshold {
                        object.insert("compact_threshold".to_owned(), JsonValue::from(threshold));
                    }
                    object
                })
                .collect()
        }),
        reasoning,
        tools: tools.tools,
        tool_choice: tools.tool_choice,
        stream: None,
    };
    Ok(PreparedRequest {
        body,
        warnings,
        tool_name_mapping: mapping,
        web_search_tool_name: tools.web_search_tool_name,
        store,
    })
}
