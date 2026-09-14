//! Request body construction (`generateContent`).

use std::collections::HashMap;

use ferrin_provider_util::reasoning::BudgetPercentages;
use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_provider_util::reasoning::map_reasoning_to_budget;
use ferrin_provider_util::reasoning::map_reasoning_to_effort;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::shared::Warning;
use serde_json::json;

use crate::capabilities::GEMINI_2_5_MAX_OUTPUT_TOKENS;
use crate::capabilities::ModelCapabilities;
use crate::capabilities::capabilities;
use crate::capabilities::max_thinking_tokens_gemini_2_5;
use crate::capabilities::minimum_thinking_level_gemini3;
use crate::config::GoogleConfig;
use crate::convert_prompt::convert_prompt;
use crate::json_schema::convert_json_schema_to_openapi_schema;
use crate::options::GoogleLanguageModelOptions;
use crate::options::ImageConfig;
use crate::options::ThinkingConfig;
use crate::options::parse_options;
use crate::prepare_tools::CODE_EXECUTION_TOOL_NAME;
use crate::prepare_tools::ids;
use crate::prepare_tools::prepare_tools;

/// Harm categories that `threshold` applies to.
pub const CONFIGURABLE_HARM_CATEGORIES: [&str; 4] = [
    "HARM_CATEGORY_HATE_SPEECH",
    "HARM_CATEGORY_DANGEROUS_CONTENT",
    "HARM_CATEGORY_HARASSMENT",
    "HARM_CATEGORY_SEXUALLY_EXPLICIT",
];

/// A request body plus everything the response mapping needs.
#[derive(Debug, Clone)]
pub struct PreparedRequest {
    /// Request body.
    pub body: JsonObject,
    /// Warnings collected while preparing.
    pub warnings: Vec<Warning>,
    /// Custom ↔ provider tool name mapping.
    pub tool_name_mapping: ToolNameMapping,
    /// Model capabilities.
    pub capabilities: ModelCapabilities,
}

/// Tool name mapping of a call (`google.code_execution` → `code_execution`).
#[must_use]
pub fn tool_name_mapping(tools: &[ToolDefinition]) -> ToolNameMapping {
    ToolNameMapping::new(
        tools,
        &HashMap::from([(ids::CODE_EXECUTION, CODE_EXECUTION_TOOL_NAME)]),
    )
}

fn insert_some<T: Into<JsonValue>>(object: &mut JsonObject, key: &str, value: Option<T>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), value.into());
    }
}

/// Resolves the `thinkingConfig` implied by `reasoning`.
fn resolve_thinking(
    reasoning: ReasoningEffort,
    model_id: &str,
    capabilities: ModelCapabilities,
    warnings: &mut Vec<Warning>,
) -> Option<ThinkingConfig> {
    if !is_custom_reasoning(reasoning) {
        return None;
    }
    if capabilities.uses_gemini3_features && !model_id.contains("gemini-3-pro-image") {
        let minimum = minimum_thinking_level_gemini3(model_id);
        let level = if reasoning == ReasoningEffort::None {
            Some(minimum)
        } else {
            map_reasoning_to_effort(
                reasoning,
                &[
                    (ReasoningEffort::Minimal, minimum),
                    (ReasoningEffort::Low, "low"),
                    (ReasoningEffort::Medium, "medium"),
                    (ReasoningEffort::High, "high"),
                    (ReasoningEffort::XHigh, "high"),
                ],
                warnings,
            )
        };
        return level.map(|level| ThinkingConfig {
            thinking_level: Some(level.to_owned()),
            ..ThinkingConfig::default()
        });
    }
    let budget = if reasoning == ReasoningEffort::None {
        Some(0)
    } else {
        map_reasoning_to_budget(
            reasoning,
            GEMINI_2_5_MAX_OUTPUT_TOKENS,
            max_thinking_tokens_gemini_2_5(model_id),
            0,
            &BudgetPercentages::default(),
            warnings,
        )
    };
    budget.map(|budget| ThinkingConfig {
        thinking_budget: Some(i64::from(budget)),
        ..ThinkingConfig::default()
    })
}

/// Drops the Vertex AI-only image configuration fields with a warning.
fn gemini_image_config(config: ImageConfig, warnings: &mut Vec<Warning>) -> ImageConfig {
    let mut dropped = Vec::new();
    if config.person_generation.is_some() {
        dropped.push("'imageConfig.personGeneration'");
    }
    if config.prominent_people.is_some() {
        dropped.push("'imageConfig.prominentPeople'");
    }
    if config.image_output_options.is_some() {
        dropped.push("'imageConfig.imageOutputOptions'");
    }
    if dropped.is_empty() {
        return config;
    }
    let verb = if dropped.len() == 1 {
        "is a Vertex AI option and is"
    } else {
        "are Vertex AI options and are"
    };
    warnings.push(Warning::other(format!(
        "{} {verb} ignored with the Gemini API",
        dropped.join(", ")
    )));
    ImageConfig {
        aspect_ratio: config.aspect_ratio,
        image_size: config.image_size,
        person_generation: None,
        prominent_people: None,
        image_output_options: None,
    }
}

fn option_warnings(
    google: &GoogleLanguageModelOptions,
    tools: &[ToolDefinition],
    warnings: &mut Vec<Warning>,
) {
    if tools.iter().any(
        |tool| matches!(tool, ToolDefinition::Provider { id, .. } if id == ids::VERTEX_RAG_STORE),
    ) {
        warnings.push(Warning::other(
            "the 'vertex_rag_store' tool is only supported with Vertex AI and may not work with the Gemini API",
        ));
    }
    if google.stream_function_call_arguments == Some(true) {
        warnings.push(Warning::other(
            "'streamFunctionCallArguments' is only supported on Vertex AI and was ignored",
        ));
    }
    if google.shared_request_type.is_some() || google.request_type.is_some() {
        warnings.push(Warning::other(
            "'sharedRequestType' and 'requestType' are Vertex AI options and were ignored",
        ));
    }
}

fn generation_config(
    options: &CallOptions,
    google: &GoogleLanguageModelOptions,
    model_id: &str,
    capabilities: ModelCapabilities,
    warnings: &mut Vec<Warning>,
) -> Result<JsonObject, ProviderError> {
    let mut config = JsonObject::new();
    insert_some(&mut config, "maxOutputTokens", options.max_output_tokens);
    insert_some(&mut config, "temperature", options.temperature);
    insert_some(&mut config, "topK", options.top_k);
    insert_some(&mut config, "topP", options.top_p);
    if options.frequency_penalty.is_some() {
        if capabilities.is_gemini_2_5 {
            warnings.push(Warning::unsupported("frequencyPenalty"));
        } else {
            insert_some(&mut config, "frequencyPenalty", options.frequency_penalty);
        }
    }
    if options.presence_penalty.is_some() {
        if capabilities.is_gemini_2_5 {
            warnings.push(Warning::unsupported("presencePenalty"));
        } else {
            insert_some(&mut config, "presencePenalty", options.presence_penalty);
        }
    }
    if let Some(stop) = &options.stop_sequences {
        config.insert("stopSequences".to_owned(), json!(stop));
    }
    insert_some(&mut config, "seed", options.seed);
    if let Some(ResponseFormat::Json { schema, .. }) = &options.response_format {
        config.insert(
            "responseMimeType".to_owned(),
            JsonValue::from("application/json"),
        );
        if let Some(schema) = schema
            && google.structured_outputs != Some(false)
            && let Some(converted) = convert_json_schema_to_openapi_schema(schema)?
        {
            config.insert("responseSchema".to_owned(), converted);
        }
    }
    insert_some(&mut config, "audioTimestamp", google.audio_timestamp);
    if let Some(modalities) = &google.response_modalities {
        config.insert("responseModalities".to_owned(), json!(modalities));
    }
    let mut thinking = google.thinking_config.clone().unwrap_or_default();
    if let Some(resolved) = resolve_thinking(options.reasoning, model_id, capabilities, warnings) {
        if resolved.thinking_level.is_some() {
            thinking.thinking_level = resolved.thinking_level;
        }
        if resolved.thinking_budget.is_some() {
            thinking.thinking_budget = resolved.thinking_budget;
        }
    }
    if thinking != ThinkingConfig::default() {
        config.insert("thinkingConfig".to_owned(), json!(thinking));
    }
    insert_some(
        &mut config,
        "mediaResolution",
        google.media_resolution.as_deref(),
    );
    if let Some(image_config) = google.image_config.clone() {
        let image_config = gemini_image_config(image_config, warnings);
        if image_config != ImageConfig::default() {
            config.insert("imageConfig".to_owned(), json!(image_config));
        }
    }
    Ok(config)
}

fn safety_settings(google: &GoogleLanguageModelOptions) -> Option<JsonValue> {
    if let Some(settings) = &google.safety_settings {
        return Some(json!(settings));
    }
    let threshold = google.threshold.as_deref()?;
    Some(JsonValue::Array(
        CONFIGURABLE_HARM_CATEGORIES
            .iter()
            .map(|category| json!({"category": category, "threshold": threshold}))
            .collect(),
    ))
}

/// Builds the `generateContent` request body for `options`.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidArgument`] for invalid provider options,
/// [`ProviderError::UnsupportedFunctionality`] for unconvertible schemas or
/// prompts and [`ProviderError::NoSuchProviderReference`] for unresolvable
/// file references.
pub fn prepare_request(
    config: &GoogleConfig,
    model_id: &str,
    options: &CallOptions,
) -> Result<PreparedRequest, ProviderError> {
    let google = parse_options(config, &options.provider_options)?;
    let capabilities = capabilities(model_id);
    let mapping = tool_name_mapping(&options.tools);
    let mut warnings = Vec::new();
    option_warnings(&google, &options.tools, &mut warnings);
    let generation = generation_config(options, &google, model_id, capabilities, &mut warnings)?;
    let prompt = convert_prompt(config, &options.prompt, capabilities, &mapping)?;
    warnings.extend(prompt.warnings);
    let tools = prepare_tools(
        &options.tools,
        options.tool_choice.as_ref(),
        capabilities,
        &mapping,
        google.retrieval_config.as_ref(),
    )?;
    warnings.extend(tools.warnings);

    let mut body = JsonObject::new();
    body.insert("generationConfig".to_owned(), JsonValue::Object(generation));
    body.insert("contents".to_owned(), JsonValue::Array(prompt.contents));
    if let Some(system) = prompt.system_instruction {
        body.insert("systemInstruction".to_owned(), JsonValue::Object(system));
    }
    if let Some(settings) = safety_settings(&google) {
        body.insert("safetySettings".to_owned(), settings);
    }
    if let Some(wire) = tools.tools {
        body.insert("tools".to_owned(), JsonValue::Array(wire));
    }
    if let Some(tool_config) = tools.tool_config {
        body.insert("toolConfig".to_owned(), JsonValue::Object(tool_config));
    }
    insert_some(&mut body, "cachedContent", google.cached_content.as_deref());
    if let Some(labels) = &google.labels {
        body.insert("labels".to_owned(), json!(labels));
    }
    insert_some(&mut body, "serviceTier", google.service_tier.as_deref());
    Ok(PreparedRequest {
        body,
        warnings,
        tool_name_mapping: mapping,
        capabilities,
    })
}
