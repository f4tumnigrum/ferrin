//! Assembly of the `POST /messages` request body.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

mod body;
mod validate;

use std::collections::BTreeSet;

use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use crate::cache_control::CacheControlValidator;
use crate::capabilities::model_capabilities;
use crate::config::AnthropicConfig;
use crate::convert_prompt::convert_prompt_with_cache;
use crate::json_schema::sanitize_json_schema;
use crate::options::ContextEdit;
use crate::options::Fallbacks;
use crate::options::parse_options;
use crate::prepare_tools::PrepareToolsSettings;
use crate::prepare_tools::prepare_tools;
use crate::prepare_tools::tool_name_mapping;

use self::body::container_value;
use self::body::context_edit;
use self::body::has_code_execution_tool;
use self::body::mcp_servers_value;
use self::validate::resolve_reasoning;
use self::validate::validate_options;

/// Name of the function tool used for JSON output on models without native
/// structured outputs.
pub const JSON_RESPONSE_TOOL_NAME: &str = "json";

/// Default thinking budget when `thinking: enabled` has none.
pub const DEFAULT_THINKING_BUDGET: u32 = 1024;

/// A prepared request.
#[derive(Debug, Clone)]
pub struct PreparedRequest {
    /// Wire body.
    pub body: JsonObject,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// Beta flags to send in `anthropic-beta`.
    pub betas: BTreeSet<String>,
    /// Whether the JSON response tool replaces native structured output.
    pub uses_json_response_tool: bool,
    /// Custom ↔ provider tool name mapping.
    pub tool_name_mapping: ToolNameMapping,
    /// Whether options were read from the configured (non-canonical) key.
    pub used_custom_key: bool,
}

/// Prepares the request body for `model_id`.
///
/// `stream` selects `stream: true` and eager tool input streaming;
/// `user_supplied_betas` are the beta flags found in configured and per-call
/// headers.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] for invalid provider options,
/// [`ProviderError::UnsupportedFunctionality`] for unsupported file media
/// types and [`ProviderError::NoSuchProviderReference`] for foreign file or
/// skill references.
#[allow(
    clippy::too_many_lines,
    reason = "mirrors the wire body field by field"
)]
pub fn prepare_request(
    config: &AnthropicConfig,
    model_id: &str,
    options: &CallOptions,
    stream: bool,
    user_supplied_betas: BTreeSet<String>,
) -> Result<PreparedRequest, ProviderError> {
    let mut warnings = Vec::new();
    for (value, feature) in [
        (options.frequency_penalty, "frequencyPenalty"),
        (options.presence_penalty, "presencePenalty"),
    ] {
        if value.is_some() {
            warnings.push(Warning::unsupported(feature));
        }
    }
    if options.seed.is_some() {
        warnings.push(Warning::unsupported("seed"));
    }
    let mut temperature = options.temperature;
    if let Some(value) = temperature {
        if value > 1.0 {
            warnings.push(Warning::unsupported_with_details(
                "temperature",
                format!("{value} exceeds anthropic maximum of 1.0. clamped to 1.0"),
            ));
            temperature = Some(1.0);
        } else if value < 0.0 {
            warnings.push(Warning::unsupported_with_details(
                "temperature",
                format!("{value} is below anthropic minimum of 0. clamped to 0"),
            ));
            temperature = Some(0.0);
        }
    }
    let json_schema = match &options.response_format {
        Some(ResponseFormat::Json { schema: None, .. }) => {
            warnings.push(Warning::unsupported_with_details(
                "responseFormat",
                "JSON response format requires a schema. The response format is ignored.",
            ));
            None
        }
        Some(ResponseFormat::Json {
            schema: Some(schema),
            ..
        }) => Some(schema.clone()),
        _ => None,
    };
    let parsed = parse_options(config, &options.provider_options)?;
    let mut anthropic = parsed.options;
    validate_options(&anthropic)?;
    let capabilities = model_capabilities(model_id);
    if !capabilities.is_known_model && options.max_output_tokens.is_none() {
        warnings.push(Warning::compatibility(
            "maxOutputTokens",
            Some(format!(
                "The model \"{model_id}\" is unknown. The max output tokens have been limited to {}. Set maxOutputTokens explicitly to override this limit.",
                capabilities.max_output_tokens
            )),
        ));
    }
    let mut top_k = options.top_k;
    let mut top_p = options.top_p;
    if capabilities.rejects_sampling_parameters {
        if temperature.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "temperature",
                format!("temperature is not supported by {model_id} and will be ignored"),
            ));
        }
        if top_k.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "topK",
                format!("topK is not supported by {model_id} and will be ignored"),
            ));
        }
        if top_p.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "topP",
                format!("topP is not supported by {model_id} and will be ignored"),
            ));
        }
    }
    let is_anthropic_model = capabilities.is_known_model || model_id.contains("claude-");
    let supports_structured_output =
        config.supports_native_structured_output && capabilities.supports_structured_output;
    let supports_strict_tools =
        config.supports_strict_tools && capabilities.supports_structured_output;
    let mode = anthropic
        .structured_output_mode
        .as_deref()
        .unwrap_or("auto");
    let use_structured_output =
        mode == "outputFormat" || (mode == "auto" && supports_structured_output);
    let json_response_tool = match &json_schema {
        Some(schema) if !use_structured_output => Some(ToolDefinition::function(
            JSON_RESPONSE_TOOL_NAME,
            Some("Respond with a JSON object.".to_owned()),
            schema.clone(),
        )),
        _ => None,
    };
    if json_response_tool.is_some() && anthropic.disable_parallel_tool_use == Some(false) {
        warnings.push(Warning::unsupported_with_details(
            "providerOptions.anthropic.disableParallelToolUse",
            "`disableParallelToolUse: false` is ignored when using the JSON response tool. Parallel tool use is disabled to ensure a single coherent JSON tool call.",
        ));
    }
    let mapping = tool_name_mapping(&options.tools);
    let mut cache = CacheControlValidator::new();
    let converted = convert_prompt_with_cache(
        config,
        &options.prompt,
        &mapping,
        anthropic.send_reasoning.unwrap_or(true),
        &mut cache,
    )?;
    warnings.extend(converted.warnings);
    let mut betas = converted.betas;

    if is_custom_reasoning(options.reasoning) && anthropic.effort.is_none() {
        let (thinking, effort) = resolve_reasoning(
            options.reasoning,
            capabilities.supports_adaptive_thinking,
            capabilities.supports_xhigh_effort,
            capabilities.max_output_tokens,
            &mut warnings,
        );
        if anthropic.thinking.is_none() {
            anthropic.thinking = thinking;
        }
        if effort.is_some()
            && anthropic
                .thinking
                .as_ref()
                .and_then(|thinking| thinking.kind.as_deref())
                != Some("disabled")
        {
            anthropic.effort = effort;
        }
    }
    let thinking_type = anthropic
        .thinking
        .as_ref()
        .and_then(|thinking| thinking.kind.clone());
    if capabilities.rejects_thinking_disabled_above_high_effort
        && thinking_type.as_deref() == Some("disabled")
        && matches!(anthropic.effort.as_deref(), Some("xhigh" | "max"))
    {
        warnings.push(Warning::unsupported_with_details(
            "providerOptions.anthropic.effort",
            format!(
                "effort '{}' is not supported by {model_id} when thinking is disabled. The effort has been lowered to 'high'.",
                anthropic.effort.as_deref().unwrap_or_default()
            ),
        ));
        anthropic.effort = Some("high".to_owned());
    }
    let is_thinking = matches!(thinking_type.as_deref(), Some("enabled" | "adaptive"));
    let block_binding = anthropic
        .thinking
        .as_ref()
        .and_then(|thinking| thinking.block_binding.clone());
    let send_thinking =
        is_thinking || thinking_type.as_deref() == Some("disabled") || block_binding.is_some();
    let mut thinking_budget = if thinking_type.as_deref() == Some("enabled") {
        anthropic
            .thinking
            .as_ref()
            .and_then(|thinking| thinking.budget_tokens)
    } else {
        None
    };
    let thinking_display = if thinking_type.as_deref() == Some("adaptive") {
        anthropic
            .thinking
            .as_ref()
            .and_then(|thinking| thinking.display.clone())
    } else {
        None
    };
    let max_tokens = options
        .max_output_tokens
        .unwrap_or(capabilities.max_output_tokens);

    let mut body = JsonObject::new();
    body.insert("model".to_owned(), JsonValue::from(model_id));
    body.insert("max_tokens".to_owned(), JsonValue::from(max_tokens));
    if is_thinking {
        if thinking_type.as_deref() == Some("enabled") && thinking_budget.is_none() {
            warnings.push(Warning::compatibility(
                "extended thinking",
                Some(
                    "thinking budget is required when thinking is enabled. using default budget of 1024 tokens."
                        .to_owned(),
                ),
            ));
            thinking_budget = Some(DEFAULT_THINKING_BUDGET);
        }
        if temperature.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "temperature",
                "temperature is not supported when thinking is enabled",
            ));
        }
        if top_k.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "topK",
                "topK is not supported when thinking is enabled",
            ));
        }
        if top_p.take().is_some() {
            warnings.push(Warning::unsupported_with_details(
                "topP",
                "topP is not supported when thinking is enabled",
            ));
        }
        body.insert(
            "max_tokens".to_owned(),
            JsonValue::from(max_tokens.saturating_add(thinking_budget.unwrap_or(0))),
        );
    } else if is_anthropic_model && top_p.is_some() && temperature.is_some() {
        warnings.push(Warning::unsupported_with_details(
            "topP",
            "topP is not supported when temperature is set. topP is ignored.",
        ));
        top_p = None;
    }
    let sent_max_tokens = body
        .get("max_tokens")
        .and_then(JsonValue::as_u64)
        .unwrap_or(u64::from(max_tokens));
    if capabilities.is_known_model && sent_max_tokens > u64::from(capabilities.max_output_tokens) {
        if options.max_output_tokens.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "maxOutputTokens",
                format!(
                    "{sent_max_tokens} (maxOutputTokens + thinkingBudget) is greater than {model_id} {} max output tokens. The max output tokens have been limited to {}.",
                    capabilities.max_output_tokens, capabilities.max_output_tokens
                ),
            ));
        }
        body.insert(
            "max_tokens".to_owned(),
            JsonValue::from(capabilities.max_output_tokens),
        );
    }
    if let Some(temperature) = temperature {
        body.insert("temperature".to_owned(), JsonValue::from(temperature));
    }
    if let Some(top_k) = top_k {
        body.insert("top_k".to_owned(), JsonValue::from(top_k));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p".to_owned(), JsonValue::from(top_p));
    }
    if let Some(stop) = &options.stop_sequences {
        body.insert("stop_sequences".to_owned(), json!(stop));
    }
    if send_thinking {
        let mut thinking = JsonObject::new();
        if let Some(kind) = &thinking_type {
            thinking.insert("type".to_owned(), JsonValue::from(kind.clone()));
        }
        if let Some(budget) = thinking_budget {
            thinking.insert("budget_tokens".to_owned(), JsonValue::from(budget));
        }
        if let Some(display) = &thinking_display {
            thinking.insert("display".to_owned(), JsonValue::from(display.clone()));
        }
        if let Some(binding) = &block_binding {
            thinking.insert(
                "block_binding".to_owned(),
                json!({"prefix_mismatch_behavior": binding.prefix_mismatch_behavior}),
            );
        }
        body.insert("thinking".to_owned(), JsonValue::Object(thinking));
    }
    let format_schema = if use_structured_output {
        json_schema.as_ref()
    } else {
        None
    };
    if anthropic.effort.is_some() || anthropic.task_budget.is_some() || format_schema.is_some() {
        let mut output_config = JsonObject::new();
        if let Some(effort) = &anthropic.effort {
            output_config.insert("effort".to_owned(), JsonValue::from(effort.clone()));
        }
        if let Some(budget) = &anthropic.task_budget {
            let mut task_budget = JsonObject::new();
            task_budget.insert("type".to_owned(), JsonValue::from(budget.kind.clone()));
            task_budget.insert("total".to_owned(), JsonValue::from(budget.total));
            if let Some(remaining) = budget.remaining {
                task_budget.insert("remaining".to_owned(), JsonValue::from(remaining));
            }
            output_config.insert("task_budget".to_owned(), JsonValue::Object(task_budget));
        }
        if let Some(schema) = format_schema {
            output_config.insert(
                "format".to_owned(),
                json!({"type": "json_schema", "schema": sanitize_json_schema(schema)}),
            );
        }
        body.insert("output_config".to_owned(), JsonValue::Object(output_config));
    }
    if let Some(speed) = &anthropic.speed {
        body.insert("speed".to_owned(), JsonValue::from(speed.clone()));
    }
    if let Some(tier) = &anthropic.service_tier {
        body.insert("service_tier".to_owned(), JsonValue::from(tier.clone()));
    }
    if let Some(geo) = &anthropic.inference_geo {
        body.insert("inference_geo".to_owned(), JsonValue::from(geo.clone()));
    }
    match &anthropic.fallbacks {
        Some(Fallbacks::Default(_)) => {
            body.insert("fallbacks".to_owned(), JsonValue::from("default"));
            betas.insert("server-side-fallback-2026-07-01".to_owned());
        }
        Some(Fallbacks::Models(models)) if !models.is_empty() => {
            body.insert(
                "fallbacks".to_owned(),
                JsonValue::Array(models.iter().cloned().map(JsonValue::Object).collect()),
            );
            betas.insert("server-side-fallback-2026-06-01".to_owned());
        }
        _ => {}
    }
    if let Some(cache) = &anthropic.cache_control {
        body.insert(
            "cache_control".to_owned(),
            serde_json::to_value(cache).map_err(ProviderError::other)?,
        );
    }
    if let Some(user_id) = anthropic
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.user_id.as_ref())
    {
        body.insert("metadata".to_owned(), json!({"user_id": user_id}));
    }
    if let Some(servers) = &anthropic.mcp_servers
        && !servers.is_empty()
    {
        betas.insert("mcp-client-2025-04-04".to_owned());
        body.insert(
            "mcp_servers".to_owned(),
            JsonValue::Array(mcp_servers_value(servers)),
        );
    }
    if let Some(container) = &anthropic.container {
        if let Some(value) = container_value(config, container)? {
            body.insert("container".to_owned(), value);
        }
        if container
            .skills
            .as_ref()
            .is_some_and(|skills| !skills.is_empty())
        {
            betas.insert("code-execution-2025-08-25".to_owned());
            betas.insert("skills-2025-10-02".to_owned());
            betas.insert("files-api-2025-04-14".to_owned());
            if !has_code_execution_tool(&options.tools) {
                warnings.push(Warning::other(
                    "code execution tool is required when using skills",
                ));
            }
        }
    }
    if let Some(system) = converted.system {
        body.insert("system".to_owned(), JsonValue::Array(system));
    }
    body.insert("messages".to_owned(), JsonValue::Array(converted.messages));
    if let Some(context) = &anthropic.context_management {
        betas.insert("context-management-2025-06-27".to_owned());
        if context
            .edits
            .iter()
            .any(|edit| matches!(edit, ContextEdit::Compact { .. }))
        {
            betas.insert("compact-2026-01-12".to_owned());
        }
        body.insert(
            "context_management".to_owned(),
            json!({"edits": context.edits.iter().map(context_edit).collect::<Vec<_>>()}),
        );
    }
    if anthropic.task_budget.is_some() {
        betas.insert("task-budgets-2026-03-13".to_owned());
    }
    if anthropic.speed.as_deref() == Some("fast") {
        betas.insert("fast-mode-2026-02-01".to_owned());
    }
    if thinking_display.as_deref() == Some("updates") {
        betas.insert("thinking-display-updates-2026-08-18".to_owned());
    }
    if block_binding.is_some() {
        betas.insert("thinking-binding-controls-2026-08-01".to_owned());
    }

    let default_eager = stream && anthropic.tool_streaming.unwrap_or(true);
    let prepared_tools = match &json_response_tool {
        Some(json_tool) => {
            let mut tools = options.tools.clone();
            tools.push(json_tool.clone());
            prepare_tools(
                config,
                &tools,
                Some(&ToolChoice::Required),
                PrepareToolsSettings {
                    disable_parallel_tool_use: true,
                    supports_structured_output: false,
                    supports_strict_tools,
                    default_eager_input_streaming: default_eager,
                },
                &mut cache,
            )
        }
        None => prepare_tools(
            config,
            &options.tools,
            options.tool_choice.as_ref(),
            PrepareToolsSettings {
                disable_parallel_tool_use: anthropic.disable_parallel_tool_use.unwrap_or(false),
                supports_structured_output,
                supports_strict_tools,
                default_eager_input_streaming: default_eager,
            },
            &mut cache,
        ),
    };
    if let Some(tools) = prepared_tools.tools {
        body.insert("tools".to_owned(), JsonValue::Array(tools));
    }
    if let Some(choice) = prepared_tools.tool_choice {
        body.insert("tool_choice".to_owned(), choice);
    }
    if stream {
        body.insert("stream".to_owned(), JsonValue::Bool(true));
    }
    warnings.extend(prepared_tools.warnings);
    warnings.extend(cache.into_warnings());
    betas.extend(prepared_tools.betas);
    betas.extend(user_supplied_betas);
    if let Some(extra) = &anthropic.anthropic_beta {
        betas.extend(extra.iter().cloned());
    }
    Ok(PreparedRequest {
        body,
        warnings,
        betas,
        uses_json_response_tool: json_response_tool.is_some(),
        tool_name_mapping: mapping,
        used_custom_key: parsed.used_custom_key,
    })
}
