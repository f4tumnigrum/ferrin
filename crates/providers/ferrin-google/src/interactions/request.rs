//! Interactions request conversion, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use std::collections::BTreeMap;

use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::ReasoningEffort;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde::Deserialize;
use serde_json::json;

use crate::config::GoogleConfig;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Options {
    pub previous_interaction_id: Option<String>,
    pub store: Option<bool>,
    pub agent: Option<String>,
    pub agent_config: Option<JsonValue>,
    pub environment: Option<JsonValue>,
    pub background: Option<bool>,
    pub polling_timeout_ms: Option<u64>,
    pub thinking_level: Option<String>,
    pub thinking_summaries: Option<String>,
    pub response_format: Option<Vec<JsonValue>>,
    pub response_modalities: Option<Vec<String>>,
    pub media_resolution: Option<String>,
    pub service_tier: Option<String>,
    pub system_instruction: Option<String>,
}

pub(super) struct Prepared {
    pub body: JsonValue,
    pub warnings: Vec<Warning>,
    pub aliases: BTreeMap<String, String>,
    pub timeout_ms: u64,
}

pub(super) fn snake_fields(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(object) => JsonValue::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let mut converted = String::new();
                    for ch in key.chars() {
                        if ch.is_uppercase() {
                            converted.push('_');
                        }
                        converted.extend(ch.to_lowercase());
                    }
                    // JSON schemas, arbitrary environment values and headers keep their keys.
                    let nested = if matches!(
                        key.as_str(),
                        "schema" | "parameters" | "headers" | "transform"
                    ) {
                        value.clone()
                    } else {
                        snake_fields(value)
                    };
                    (converted, nested)
                })
                .collect(),
        ),
        JsonValue::Array(values) => JsonValue::Array(values.iter().map(snake_fields).collect()),
        _ => value.clone(),
    }
}

pub(super) fn prepare(
    config: &GoogleConfig,
    model: &str,
    call: &CallOptions,
) -> Result<Prepared, ProviderError> {
    let mut merged = call
        .provider_options
        .get("google")
        .cloned()
        .unwrap_or_default();
    if config.options_key() != "google"
        && let Some(custom) = call.provider_options.get(config.options_key())
    {
        merged.extend(custom.clone());
    }
    let options: Options = serde_json::from_value(JsonValue::Object(merged))
        .map_err(|_| super::invalid("provider_options", "invalid Google interactions options"))?;
    if options.polling_timeout_ms == Some(0) {
        return Err(super::invalid(
            "pollingTimeoutMs",
            "polling timeout must be positive",
        ));
    }
    let mut warnings = Vec::new();
    let mut body = json!({});
    body[if options.agent.is_some() {
        "agent"
    } else {
        "model"
    }] = json!(options.agent.as_deref().unwrap_or(model));
    let (input, system) = super::prompt::convert(config, call, &options, &mut warnings)?;
    body["input"] = json!(input);
    if system.is_some() && options.system_instruction.is_some() {
        warnings.push(Warning::unsupported_with_details(
            "systemInstruction",
            "prompt system messages take precedence",
        ));
    }
    if let Some(system) = system.or(options.system_instruction.clone()) {
        body["system_instruction"] = json!(system);
    }
    for (key, value) in [
        (
            "previous_interaction_id",
            json!(options.previous_interaction_id),
        ),
        ("store", json!(options.store)),
        ("background", json!(options.background)),
        ("response_modalities", json!(options.response_modalities)),
        ("service_tier", json!(options.service_tier)),
    ] {
        if !value.is_null() {
            body[key] = value;
        }
    }
    let mut generation = json!({});
    for (key, value) in [
        ("temperature", json!(call.temperature)),
        ("top_p", json!(call.top_p)),
        ("top_k", json!(call.top_k)),
        ("seed", json!(call.seed)),
        ("stop_sequences", json!(call.stop_sequences)),
        ("max_output_tokens", json!(call.max_output_tokens)),
        ("thinking_level", json!(options.thinking_level)),
        ("thinking_summaries", json!(options.thinking_summaries)),
    ] {
        if !value.is_null() {
            generation[key] = value;
        }
    }
    if generation.get("thinking_level").is_none() {
        let effort = match call.reasoning {
            ReasoningEffort::ProviderDefault => None,
            ReasoningEffort::None | ReasoningEffort::Minimal => Some("minimal"),
            ReasoningEffort::Low => Some("low"),
            ReasoningEffort::Medium => Some("medium"),
            ReasoningEffort::High | ReasoningEffort::XHigh => Some("high"),
        };
        if let Some(effort) = effort {
            generation["thinking_level"] = json!(effort);
        }
    }
    for (key, present) in [
        ("frequencyPenalty", call.frequency_penalty.is_some()),
        ("presencePenalty", call.presence_penalty.is_some()),
    ] {
        if present {
            warnings.push(Warning::unsupported(key));
        }
    }
    let mut tools = Vec::new();
    let mut aliases = BTreeMap::new();
    for tool in &call.tools {
        match tool {
            ToolDefinition::Function {
                name,
                description,
                input_schema,
                ..
            } => {
                tools.push(json!({"type":"function", "name":name, "description":description.as_deref().unwrap_or(""), "parameters":input_schema}));
            }
            ToolDefinition::Provider { id, name, args } => {
                let kind = id.strip_prefix("google.").unwrap_or("");
                if !matches!(
                    kind,
                    "google_search"
                        | "code_execution"
                        | "url_context"
                        | "file_search"
                        | "google_maps"
                        | "computer_use"
                        | "retrieval"
                ) {
                    warnings.push(Warning::unsupported(format!("provider tool {id}")));
                    continue;
                }
                let mut tool = snake_fields(&json!(args));
                tool["type"] = json!(kind);
                if kind == "google_search"
                    && let Some(search) = args.get("searchTypes")
                {
                    let types: Vec<_> =
                        [("webSearch", "web_search"), ("imageSearch", "image_search")]
                            .into_iter()
                            .filter_map(|(key, wire)| search.get(key).map(|_| wire))
                            .collect();
                    tool["search_types"] = json!(types);
                }
                aliases.insert(kind.to_owned(), name.as_str().to_owned());
                tools.push(tool);
            }
            _ => warnings.push(Warning::unsupported("tool definition")),
        }
    }
    if tools.iter().any(|tool| tool["type"] == "function")
        && let Some(choice) = &call.tool_choice
    {
        generation["tool_choice"] = match choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("any"),
            ToolChoice::Tool { tool_name } => {
                json!({"allowed_tools":{"mode":"validated", "tools":[tool_name]}})
            }
        };
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    let mut formats: Vec<_> = options
        .response_format
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(snake_fields)
        .collect();
    if let Some(ResponseFormat::Json { schema, .. }) = &call.response_format {
        let mut format = json!({"type":"text", "mime_type":"application/json"});
        if let Some(schema) = schema {
            format["schema"] = schema.clone();
        }
        formats.insert(0, format);
    }
    if options.agent.is_some() {
        if generation
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            warnings.push(Warning::unsupported("agent generation settings"));
        }
        if !formats.is_empty() {
            warnings.push(Warning::unsupported("agent response format"));
        }
        if let Some(value) = options.agent_config {
            body["agent_config"] = snake_fields(&value);
        }
        if let Some(value) = options.environment {
            body["environment"] = value;
        }
    } else {
        if generation
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            body["generation_config"] = generation;
        }
        if !formats.is_empty() {
            body["response_format"] = json!(formats);
        }
        if options.environment.is_some() || options.agent_config.is_some() {
            warnings.push(Warning::unsupported("model agent configuration"));
        }
    }
    Ok(Prepared {
        body,
        warnings,
        aliases,
        timeout_ms: options.polling_timeout_ms.unwrap_or(1_800_000),
    })
}
