//! Provider options read from `provider_options["google"]` (and the
//! configured provider name when it differs).

use std::collections::BTreeMap;

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::error::InvalidArgumentError;
use serde::Deserialize;
use serde::Serialize;

use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::GoogleConfig;

/// Thinking configuration (`thinkingConfig`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Thinking budget in tokens (Gemini 2.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    /// Whether thought summaries are returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    /// Thinking level (`minimal`, `low`, `medium`, `high`; Gemini 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// A safety setting (`{category, threshold}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetySetting {
    /// Harm category (`HARM_CATEGORY_HATE_SPEECH`, ...).
    pub category: String,
    /// Block threshold (`BLOCK_MEDIUM_AND_ABOVE`, `BLOCK_NONE`, `OFF`, ...).
    pub threshold: String,
}

/// Image generation configuration (`imageConfig`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    /// Aspect ratio (`1:1`, `16:9`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// Image size (`1K`, `2K`, `4K`, `512`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
    /// Person generation policy (Vertex AI only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_generation: Option<String>,
    /// Prominent people policy (Vertex AI only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prominent_people: Option<String>,
    /// Output options (Vertex AI only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_output_options: Option<JsonObject>,
}

/// Language model options (`provider_options["google"]`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleLanguageModelOptions {
    /// Response modalities (`TEXT`, `IMAGE`).
    #[serde(default)]
    pub response_modalities: Option<Vec<String>>,
    /// Thinking configuration.
    #[serde(default)]
    pub thinking_config: Option<ThinkingConfig>,
    /// Name of a cached content resource.
    #[serde(default)]
    pub cached_content: Option<String>,
    /// Whether JSON response formats send `responseSchema` (default `true`).
    #[serde(default)]
    pub structured_outputs: Option<bool>,
    /// Explicit safety settings.
    #[serde(default)]
    pub safety_settings: Option<Vec<SafetySetting>>,
    /// Threshold applied to the four configurable harm categories.
    #[serde(default)]
    pub threshold: Option<String>,
    /// Whether audio timestamps are understood.
    #[serde(default)]
    pub audio_timestamp: Option<bool>,
    /// Request labels.
    #[serde(default)]
    pub labels: Option<BTreeMap<String, String>>,
    /// Media resolution (`MEDIA_RESOLUTION_LOW`, ...).
    #[serde(default)]
    pub media_resolution: Option<String>,
    /// Image generation configuration.
    #[serde(default)]
    pub image_config: Option<ImageConfig>,
    /// Retrieval configuration (`{latLng: {latitude, longitude}}`) added to
    /// `toolConfig`.
    #[serde(default)]
    pub retrieval_config: Option<JsonObject>,
    /// Vertex AI only: stream function call arguments.
    #[serde(default)]
    pub stream_function_call_arguments: Option<bool>,
    /// Service tier (`standard`, `flex`, `priority`).
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Vertex AI only: shared request type header.
    #[serde(default)]
    pub shared_request_type: Option<String>,
    /// Vertex AI only: request type header.
    #[serde(default)]
    pub request_type: Option<String>,
}

impl GoogleLanguageModelOptions {
    /// Overlays `other` on `self`: every option set in `other` wins.
    fn merge(mut self, other: Self) -> Self {
        macro_rules! take {
            ($($field:ident),* $(,)?) => {
                $( if other.$field.is_some() { self.$field = other.$field; } )*
            };
        }
        take!(
            response_modalities,
            thinking_config,
            cached_content,
            structured_outputs,
            safety_settings,
            threshold,
            audio_timestamp,
            labels,
            media_resolution,
            image_config,
            retrieval_config,
            stream_function_call_arguments,
            service_tier,
            shared_request_type,
            request_type,
        );
        self
    }
}

/// Parses `T` under the canonical `google` key and, when the configured name
/// differs, under that name as well; `merge` combines the two (the custom
/// key wins).
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] when either object does not match the
/// option schema.
pub fn parse_merged<T: serde::de::DeserializeOwned + Default>(
    config: &GoogleConfig,
    provider_options: &ProviderOptions,
    merge: impl FnOnce(T, T) -> T,
) -> Result<T, InvalidArgumentError> {
    let canonical =
        parse_provider_options::<T>(CANONICAL_OPTIONS_KEY, provider_options)?.unwrap_or_default();
    let key = config.options_key();
    if key == CANONICAL_OPTIONS_KEY {
        return Ok(canonical);
    }
    match parse_provider_options::<T>(key, provider_options)? {
        Some(custom) => Ok(merge(canonical, custom)),
        None => Ok(canonical),
    }
}

/// Parses the language model options (canonical key, then configured name).
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] when either object does not match the
/// option schema.
pub fn parse_options(
    config: &GoogleConfig,
    provider_options: &ProviderOptions,
) -> Result<GoogleLanguageModelOptions, InvalidArgumentError> {
    parse_merged(config, provider_options, GoogleLanguageModelOptions::merge)
}

/// Reads the raw option object of a part or message: the configured name
/// first, then the canonical key.
#[must_use]
pub fn part_options<'a>(
    config: &GoogleConfig,
    provider_options: Option<&'a ProviderOptions>,
) -> Option<&'a JsonObject> {
    let options = provider_options?;
    options
        .get(config.options_key())
        .or_else(|| options.get(CANONICAL_OPTIONS_KEY))
}

/// Part-level options and metadata (`thoughtSignature`, `thought`,
/// `serverToolCallId`, `serverToolType`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartOptions {
    /// Thought signature returned with a previous response part.
    #[serde(default)]
    pub thought_signature: Option<String>,
    /// Marks an assistant file as a thought (reasoning file).
    #[serde(default)]
    pub thought: Option<bool>,
    /// Id of a server-side tool call.
    #[serde(default)]
    pub server_tool_call_id: Option<String>,
    /// Type of a server-side tool call.
    #[serde(default)]
    pub server_tool_type: Option<String>,
}

/// Reads the part options of a prompt part.
#[must_use]
pub fn read_part_options(
    config: &GoogleConfig,
    provider_options: Option<&ProviderOptions>,
) -> PartOptions {
    read_options(part_options(config, provider_options)).unwrap_or_default()
}

/// Deserializes `object` into `T`, ignoring unknown keys.
#[must_use]
pub fn read_options<T: serde::de::DeserializeOwned>(object: Option<&JsonObject>) -> Option<T> {
    let object = object?;
    serde_json::from_value(JsonValue::Object(object.clone())).ok()
}
