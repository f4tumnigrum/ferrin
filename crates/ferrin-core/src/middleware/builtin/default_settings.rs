//! Default call settings.

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::ResponseFormat;

use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;

/// Settings applied when the call does not set them.
///
/// Scalars fill `None` values. `tools` applies when the call sends no
/// tools. `headers` and `provider_options` are merged with the call's
/// values taking precedence (provider options merge recursively).
#[derive(Debug, Clone, Default)]
pub struct CallDefaults {
    /// See [`CallOptions::max_output_tokens`].
    pub max_output_tokens: Option<u32>,
    /// See [`CallOptions::temperature`].
    pub temperature: Option<f64>,
    /// See [`CallOptions::stop_sequences`].
    pub stop_sequences: Option<Vec<String>>,
    /// See [`CallOptions::top_p`].
    pub top_p: Option<f64>,
    /// See [`CallOptions::top_k`].
    pub top_k: Option<u32>,
    /// See [`CallOptions::presence_penalty`].
    pub presence_penalty: Option<f64>,
    /// See [`CallOptions::frequency_penalty`].
    pub frequency_penalty: Option<f64>,
    /// See [`CallOptions::response_format`].
    pub response_format: Option<ResponseFormat>,
    /// See [`CallOptions::seed`].
    pub seed: Option<u64>,
    /// See [`CallOptions::tools`].
    pub tools: Vec<ToolDefinition>,
    /// See [`CallOptions::tool_choice`].
    pub tool_choice: Option<ToolChoice>,
    /// See [`CallOptions::headers`].
    pub headers: Headers,
    /// See [`CallOptions::provider_options`].
    pub provider_options: ProviderOptions,
}

/// Middleware created by [`default_settings`].
#[derive(Debug, Clone)]
pub struct DefaultSettings {
    defaults: CallDefaults,
}

/// Fills unset call options from `defaults`.
#[must_use]
pub fn default_settings(defaults: CallDefaults) -> DefaultSettings {
    DefaultSettings { defaults }
}

impl DefaultSettings {
    /// Applies the defaults to `options`.
    #[must_use]
    pub fn apply(&self, mut options: CallOptions) -> CallOptions {
        let defaults = &self.defaults;
        macro_rules! fill {
            ($($field:ident),* $(,)?) => {
                $(
                    if options.$field.is_none() {
                        options.$field = defaults.$field.clone();
                    }
                )*
            };
        }
        fill!(
            max_output_tokens,
            temperature,
            stop_sequences,
            top_p,
            top_k,
            presence_penalty,
            frequency_penalty,
            response_format,
            seed,
            tool_choice,
        );
        if let (Some(default), Some(response_format)) =
            (&defaults.response_format, &mut options.response_format)
        {
            merge_response_format(default, response_format);
        }
        if options.tools.is_empty() && !defaults.tools.is_empty() {
            options.tools = defaults.tools.clone();
        }
        if !defaults.headers.is_empty() {
            let mut headers = defaults.headers.clone();
            headers.merge(&options.headers);
            options.headers = headers;
        }
        if !defaults.provider_options.is_empty() {
            options.provider_options =
                merge_provider_options(&defaults.provider_options, options.provider_options);
        }
        options
    }
}

fn merge_response_format(default: &ResponseFormat, response_format: &mut ResponseFormat) {
    if let (
        ResponseFormat::Json {
            schema: base_schema,
            name: base_name,
            description: base_description,
        },
        ResponseFormat::Json {
            schema,
            name,
            description,
        },
    ) = (default, response_format)
    {
        if schema.is_none() {
            *schema = base_schema.clone();
        } else if let (Some(JsonValue::Object(base)), Some(JsonValue::Object(overrides))) =
            (base_schema, schema.as_mut())
        {
            *overrides = merge_json_objects(base, std::mem::take(overrides));
        }
        if name.is_none() {
            *name = base_name.clone();
        }
        if description.is_none() {
            *description = base_description.clone();
        }
    }
}

impl LanguageModelMiddleware for DefaultSettings {
    fn transform_params<'a>(
        &'a self,
        options: CallOptions,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        let options = self.apply(options);
        Box::pin(async move { Ok(options) })
    }
}

pub(crate) fn merge_provider_options(
    base: &ProviderOptions,
    overrides: ProviderOptions,
) -> ProviderOptions {
    let mut merged = base.clone();
    for (provider, options) in overrides {
        if matches!(provider.as_str(), "__proto__" | "constructor" | "prototype") {
            continue;
        }
        match merged.remove(&provider) {
            Some(existing) => {
                merged.insert(provider, merge_json_objects(&existing, options));
            }
            None => {
                merged.insert(provider, options);
            }
        }
    }
    merged
}

/// Deeply merges two JSON objects: keys of `overrides` win, except that
/// nested objects on both sides are merged recursively. Arrays and scalars
/// (including `null`) override.
#[must_use]
pub fn merge_json_objects(base: &JsonObject, overrides: JsonObject) -> JsonObject {
    let mut merged = base.clone();
    for (key, value) in overrides {
        if matches!(key.as_str(), "__proto__" | "constructor" | "prototype") {
            continue;
        }
        match (merged.remove(&key), value) {
            (Some(JsonValue::Object(existing)), JsonValue::Object(incoming)) => {
                merged.insert(
                    key,
                    JsonValue::Object(merge_json_objects(&existing, incoming)),
                );
            }
            (_, value) => {
                merged.insert(key, value);
            }
        }
    }
    merged
}
