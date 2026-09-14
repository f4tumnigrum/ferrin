//! Provider option keys: the configured name, its camelCase variant and the
//! shared `openaiCompatible` key.

use ferrin_spec::JsonObject;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;

/// Key every OpenAI-compatible model reads in addition to the provider name.
pub const SHARED_OPTIONS_KEY: &str = "openaiCompatible";

/// Former spelling of [`SHARED_OPTIONS_KEY`]; still read, with a
/// deprecation warning.
pub const DEPRECATED_SHARED_OPTIONS_KEY: &str = "openai-compatible";

/// Converts `snake_case` / `kebab-case` to `camelCase`: a `_` or `-`
/// followed by an ASCII lowercase letter is replaced by the uppercase
/// letter (`my-provider` → `myProvider`); everything else is kept.
#[must_use]
pub fn to_camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '_' || c == '-')
            && let Some(next) = chars.peek().copied()
            && next.is_ascii_lowercase()
        {
            out.push(next.to_ascii_uppercase());
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Key under which provider metadata is written: the camelCase variant of
/// `name` when the caller supplied options under it, otherwise `name`.
#[must_use]
pub fn resolve_metadata_key(name: &str, provider_options: &ProviderOptions) -> String {
    let camel = to_camel_case(name);
    if camel != name && provider_options.contains_key(&camel) {
        return camel;
    }
    name.to_owned()
}

/// Warns when options are supplied under the raw (non-camelCase) name.
pub fn warn_if_deprecated_key(
    name: &str,
    provider_options: &ProviderOptions,
    warnings: &mut Vec<Warning>,
) {
    let camel = to_camel_case(name);
    if camel != name && provider_options.contains_key(name) {
        warnings.push(Warning::deprecated(
            format!("providerOptions key '{name}'"),
            format!("Use '{camel}' instead."),
        ));
    }
}

/// Keys read for a model of provider `name`, in precedence order (later
/// entries override earlier ones): the deprecated shared key, the shared
/// key, the raw name and its camelCase variant.
#[must_use]
pub fn option_keys(name: &str) -> Vec<String> {
    let mut keys = vec![
        DEPRECATED_SHARED_OPTIONS_KEY.to_owned(),
        SHARED_OPTIONS_KEY.to_owned(),
        name.to_owned(),
    ];
    let camel = to_camel_case(name);
    if camel != name {
        keys.push(camel);
    }
    keys
}

/// Merges the option objects found under `keys`, later keys overriding
/// earlier ones; returns `None` when no key is present.
#[must_use]
pub fn merged_options(keys: &[String], provider_options: &ProviderOptions) -> Option<JsonObject> {
    let mut merged: Option<JsonObject> = None;
    for key in keys {
        if let Some(object) = provider_options.get(key) {
            merged
                .get_or_insert_with(JsonObject::new)
                .extend(object.clone());
        }
    }
    merged
}

/// Entries of the option objects under the raw name and its camelCase
/// variant whose key is not in `known`; passed through to the request body.
#[must_use]
pub fn passthrough_options(
    name: &str,
    provider_options: &ProviderOptions,
    known: &[&str],
) -> JsonObject {
    let mut out = JsonObject::new();
    let camel = to_camel_case(name);
    let mut keys = vec![name];
    if camel != name {
        keys.push(camel.as_str());
    }
    for key in keys {
        if let Some(object) = provider_options.get(key) {
            for (field, value) in object {
                if !known.contains(&field.as_str()) {
                    out.insert(field.clone(), value.clone());
                }
            }
        }
    }
    out
}

/// Extra wire fields of a message or part: the object under the shared key
/// of its `provider_options`.
#[must_use]
pub fn shared_extra_fields(provider_options: Option<&ProviderOptions>) -> JsonObject {
    provider_options
        .and_then(|options| options.get(SHARED_OPTIONS_KEY))
        .cloned()
        .unwrap_or_default()
}
