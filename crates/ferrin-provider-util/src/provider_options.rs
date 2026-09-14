//! Provider option parsing.

use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::error::InvalidArgumentError;
use serde::de::DeserializeOwned;

/// Extracts and deserializes the options addressed to `provider_key`.
///
/// Returns `None` when the key is absent.
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] (argument `provider_options`) when the
/// options do not deserialize into `T`.
pub fn parse_provider_options<T: DeserializeOwned>(
    provider_key: &str,
    options: &ProviderOptions,
) -> Result<Option<T>, InvalidArgumentError> {
    let Some(raw) = options.get(provider_key) else {
        return Ok(None);
    };
    serde_json::from_value::<T>(JsonValue::Object(raw.clone()))
        .map(Some)
        .map_err(|error| {
            let mut invalid = InvalidArgumentError::new(
                "provider_options",
                format!("invalid {provider_key} provider options: {error}"),
            );
            invalid.cause = Some(Box::new(error));
            invalid
        })
}
