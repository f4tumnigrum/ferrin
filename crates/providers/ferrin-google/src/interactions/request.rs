//! Interactions request conversion, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use std::collections::BTreeMap;

use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ToolChoice;
use ferrin_spec::language_model::ToolDefinition;
use serde::Deserialize;
use serde_json::json;

use crate::config::GoogleConfig;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub image_config: Option<JsonValue>,
    pub response_modalities: Option<Vec<String>>,
    pub media_resolution: Option<String>,
    pub service_tier: Option<String>,
    pub system_instruction: Option<String>,
    #[serde(rename = "signature")]
    _signature: Option<String>,
    #[serde(rename = "interactionId")]
    _interaction_id: Option<String>,
}

pub(super) struct Prepared {
    pub body: JsonValue,
    pub warnings: Vec<Warning>,
    pub aliases: BTreeMap<String, String>,
    pub timeout_ms: u64,
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
    let merged = JsonValue::Object(merged);
    string_field(
        &merged,
        "thinkingLevel",
        &["minimal", "low", "medium", "high"],
    )?;
    string_field(&merged, "thinkingSummaries", &["auto", "none"])?;
    string_field(
        &merged,
        "mediaResolution",
        &["low", "medium", "high", "ultra_high"],
    )?;
    string_field(&merged, "serviceTier", &["flex", "standard", "priority"])?;
    if let Some(modalities) = merged
        .get("responseModalities")
        .filter(|value| !value.is_null())
        && !modalities.as_array().is_some_and(|values| {
            values.iter().all(|value| {
                value.as_str().is_some_and(|value| {
                    ["text", "image", "audio", "video", "document"].contains(&value)
                })
            })
        })
    {
        return Err(super::invalid(
            "responseModalities",
            "invalid response modality",
        ));
    }
    let options: Options = serde_json::from_value(merged)
        .map_err(|_| super::invalid("provider_options", "invalid Google interactions options"))?;
    if options.polling_timeout_ms == Some(0) {
        return Err(super::invalid(
            "pollingTimeoutMs",
            "polling timeout must be positive",
        ));
    }
    let normalized_agent_config = options
        .agent_config
        .as_ref()
        .map(agent_config)
        .transpose()?;
    let normalized_environment = options.environment.as_ref().map(environment).transpose()?;
    if let Some(config) = &options.image_config {
        if !config.is_object() {
            return Err(super::invalid(
                "imageConfig",
                "imageConfig must be an object",
            ));
        }
        let format = json!({"type":"image","aspectRatio":config.get("aspectRatio"),"imageSize":config.get("imageSize")});
        response_format(&format)?;
    }
    let mut warnings = Vec::new();
    let mut prompt_warnings = Vec::new();
    let mut body = json!({});
    body[if options.agent.is_some() {
        "agent"
    } else {
        "model"
    }] = json!(options.agent.as_deref().unwrap_or(model));
    let (input, system) = super::prompt::convert(config, call, &options, &mut prompt_warnings)?;
    body["input"] = json!(input);
    if system.is_some() && options.system_instruction.is_some() {
        prompt_warnings.push(Warning::other("google.interactions: both AI SDK system message and providerOptions.google.systemInstruction were set; using the AI SDK system message."));
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
        ("max_output_tokens", json!(call.max_output_tokens)),
        ("thinking_level", json!(options.thinking_level)),
        ("thinking_summaries", json!(options.thinking_summaries)),
    ] {
        if !value.is_null() {
            generation[key] = value;
        }
    }
    if let Some(sequences) = call
        .stop_sequences
        .as_ref()
        .filter(|sequences| !sequences.is_empty())
    {
        generation["stop_sequences"] = json!(sequences);
    }
    for (key, present) in [
        ("frequencyPenalty", call.frequency_penalty.is_some()),
        ("presencePenalty", call.presence_penalty.is_some()),
    ] {
        if present && options.agent.is_none() {
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
                let Some(tool) = super::tools::provider_tool(kind, args) else {
                    warnings.push(Warning::unsupported_with_details(
                        format!("provider-defined tool {id}"),
                        format!("provider-defined tool {id} is not supported by google.interactions; tool dropped."),
                    ));
                    continue;
                };
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
    if options.agent.is_some() && matches!(call.response_format, Some(ResponseFormat::Json { .. }))
    {
        warnings.push(Warning::other("google.interactions: structured output (responseFormat) is not supported when an agent is set; responseFormat will be ignored."));
    }
    let mut formats: Vec<_> = options
        .response_format
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(response_format)
        .collect::<Result<_, _>>()?;
    if options.agent.is_none()
        && let Some(ResponseFormat::Json { schema, .. }) = &call.response_format
    {
        let mut format = json!({"type":"text", "mime_type":"application/json"});
        if let Some(schema) = schema {
            format["schema"] = schema.clone();
        }
        formats.insert(0, format);
    }
    warnings.extend(prompt_warnings);
    if options.agent.is_none()
        && let Some(config) = &options.image_config
    {
        if !config.is_object() {
            return Err(super::invalid(
                "imageConfig",
                "imageConfig must be an object",
            ));
        }
        let has_image_format = formats.iter().any(|format| format["type"] == "image");
        warnings.push(Warning::other(if has_image_format {
            "google.interactions: providerOptions.google.imageConfig is deprecated and was ignored because providerOptions.google.responseFormat already supplies an image entry. Use responseFormat exclusively."
        } else {
            "google.interactions: providerOptions.google.imageConfig is deprecated. Use providerOptions.google.responseFormat with a { type: \"image\", ... } entry instead."
        }));
        if !has_image_format {
            let mut format = json!({"type":"image", "mime_type":"image/png"});
            for (key, wire) in [("aspectRatio", "aspect_ratio"), ("imageSize", "image_size")] {
                if let Some(value) = config.get(key).filter(|value| !value.is_null()) {
                    format[wire] = value.clone();
                }
            }
            formats.push(format);
        }
    }
    if options.agent.is_some() {
        let dropped: Vec<_> = [
            ("temperature", call.temperature.is_some()),
            ("topP", call.top_p.is_some()),
            ("topK", call.top_k.is_some()),
            ("frequencyPenalty", call.frequency_penalty.is_some()),
            ("presencePenalty", call.presence_penalty.is_some()),
            ("seed", call.seed.is_some()),
            (
                "stopSequences",
                call.stop_sequences
                    .as_ref()
                    .is_some_and(|values| !values.is_empty()),
            ),
            ("maxOutputTokens", call.max_output_tokens.is_some()),
            ("thinkingLevel", options.thinking_level.is_some()),
            ("thinkingSummaries", options.thinking_summaries.is_some()),
            ("imageConfig", options.image_config.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect();
        if !dropped.is_empty() {
            let fields = dropped.join(", ");
            let verb = if dropped.len() == 1 { "is" } else { "are" };
            warnings.push(Warning::other(format!("google.interactions: {fields} {verb} not supported when an agent is set; use providerOptions.google.agentConfig instead. Dropped from the request body.")));
        }
        if let Some(value) = normalized_agent_config {
            body["agent_config"] = value;
        }
        if let Some(value) = normalized_environment {
            body["environment"] = value;
        }
    } else {
        if generation
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            body["generation_config"] = generation;
        }
        if options.environment.is_some() {
            warnings.push(Warning::other("google.interactions: environment is only supported when an agent is set; environment will be omitted from the request body."));
        }
    }
    if !formats.is_empty() {
        body["response_format"] = json!(formats);
    }
    Ok(Prepared {
        body,
        warnings,
        aliases,
        timeout_ms: options.polling_timeout_ms.unwrap_or(1_800_000),
    })
}

fn optional_fields(value: &JsonValue, fields: &[(&str, &str)]) -> JsonValue {
    let mut output = json!({});
    for (key, wire) in fields {
        if let Some(value) = value.get(key).filter(|value| !value.is_null()) {
            output[wire] = value.clone();
        }
    }
    output
}

fn string_field(value: &JsonValue, key: &str, allowed: &[&str]) -> Result<(), ProviderError> {
    if let Some(value) = value.get(key).filter(|value| !value.is_null())
        && !value
            .as_str()
            .is_some_and(|value| allowed.is_empty() || allowed.contains(&value))
    {
        return Err(super::invalid(key, "invalid interactions option value"));
    }
    Ok(())
}

fn response_format(value: &JsonValue) -> Result<JsonValue, ProviderError> {
    let fields: &[(&str, &str)] = match value["type"].as_str() {
        Some("text") => &[
            ("type", "type"),
            ("mimeType", "mime_type"),
            ("schema", "schema"),
        ],
        Some("image") => {
            string_field(
                value,
                "aspectRatio",
                &[
                    "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9", "1:8",
                    "8:1", "1:4", "4:1",
                ],
            )?;
            string_field(value, "imageSize", &["1K", "2K", "4K", "512"])?;
            &[
                ("type", "type"),
                ("mimeType", "mime_type"),
                ("aspectRatio", "aspect_ratio"),
                ("imageSize", "image_size"),
            ]
        }
        Some("audio") => &[("type", "type"), ("mimeType", "mime_type")],
        Some("video") => {
            string_field(value, "aspectRatio", &["16:9", "9:16"])?;
            string_field(value, "resolution", &["360p", "720p", "1080p", "4k"])?;
            string_field(value, "duration", &[])?;
            string_field(value, "delivery", &["inline", "uri"])?;
            string_field(value, "gcsUri", &[])?;
            &[
                ("type", "type"),
                ("aspectRatio", "aspect_ratio"),
                ("resolution", "resolution"),
                ("duration", "duration"),
                ("delivery", "delivery"),
                ("gcsUri", "gcs_uri"),
            ]
        }
        _ => {
            return Err(super::invalid(
                "responseFormat",
                "invalid response format type",
            ));
        }
    };
    if value["type"] != "video" {
        string_field(value, "mimeType", &[])?;
    }
    Ok(optional_fields(value, fields))
}

fn agent_config(value: &JsonValue) -> Result<JsonValue, ProviderError> {
    match value["type"].as_str() {
        Some("dynamic") => Ok(json!({"type":"dynamic"})),
        Some("deep-research") => {
            string_field(value, "thinkingSummaries", &["auto", "none"])?;
            string_field(value, "visualization", &["off", "auto"])?;
            if value
                .get("collaborativePlanning")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(super::invalid(
                    "agentConfig",
                    "collaborativePlanning must be boolean",
                ));
            }
            Ok(optional_fields(
                value,
                &[
                    ("type", "type"),
                    ("thinkingSummaries", "thinking_summaries"),
                    ("visualization", "visualization"),
                    ("collaborativePlanning", "collaborative_planning"),
                ],
            ))
        }
        _ => Err(super::invalid(
            "agentConfig",
            "invalid agent configuration type",
        )),
    }
}

fn environment(value: &JsonValue) -> Result<JsonValue, ProviderError> {
    if value.is_string() {
        return Ok(value.clone());
    }
    if value["type"] != "remote" {
        return Err(super::invalid("environment", "invalid remote environment"));
    }
    let mut output = json!({"type":"remote"});
    if let Some(sources) = value.get("sources").filter(|value| !value.is_null()) {
        let sources = sources
            .as_array()
            .ok_or_else(|| super::invalid("environment", "sources must be an array"))?;
        let sources = sources
            .iter()
            .map(|source| {
                let required: &[&str] = match source["type"].as_str() {
                    Some("inline") => &["content", "target"],
                    Some("gcs" | "repository") => &["source"],
                    _ => return Err(super::invalid("environment", "invalid source type")),
                };
                if required.iter().any(|key| !source[key].is_string()) {
                    return Err(super::invalid(
                        "environment",
                        "source requires string content or path",
                    ));
                }
                string_field(source, "target", &[])?;
                Ok(if source["type"] == "inline" {
                    optional_fields(
                        source,
                        &[
                            ("type", "type"),
                            ("content", "content"),
                            ("target", "target"),
                        ],
                    )
                } else {
                    optional_fields(
                        source,
                        &[("type", "type"), ("source", "source"), ("target", "target")],
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !sources.is_empty() {
            output["sources"] = json!(sources);
        }
    }
    if let Some(network) = value.get("network").filter(|value| !value.is_null()) {
        output["network"] = if network == "disabled" {
            json!("disabled")
        } else {
            let allowlist = network["allowlist"]
                .as_array()
                .ok_or_else(|| super::invalid("environment", "network requires an allowlist"))?;
            let allowlist = allowlist
                .iter()
                .map(|entry| {
                    if !entry["domain"].is_string() {
                        return Err(super::invalid(
                            "environment",
                            "allowlist domain must be a string",
                        ));
                    }
                    if let Some(transform) = entry.get("transform").filter(|value| !value.is_null())
                        && !transform.as_array().is_some_and(|values| {
                            values.iter().all(|value| {
                                value
                                    .as_object()
                                    .is_some_and(|value| value.values().all(JsonValue::is_string))
                            })
                        })
                    {
                        return Err(super::invalid("environment", "invalid network transform"));
                    }
                    Ok(optional_fields(
                        entry,
                        &[("domain", "domain"), ("transform", "transform")],
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            json!({"allowlist":allowlist})
        };
    }
    Ok(output)
}
