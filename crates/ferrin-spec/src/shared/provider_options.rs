//! Provider-scoped option and metadata maps.

use std::collections::BTreeMap;

use crate::json::JsonObject;

/// Provider-specific request options, grouped by provider key.
///
/// The outer key is the provider options name (for example `openai` or
/// `anthropic`); the inner object is interpreted by that provider only.
/// Adapters must ignore keys that belong to other providers.
pub type ProviderOptions = BTreeMap<String, JsonObject>;

/// Provider-specific response metadata, grouped by provider key.
///
/// Same shape as [`ProviderOptions`]; produced by adapters to expose raw
/// provider information that has no standardized field.
pub type ProviderMetadata = BTreeMap<String, JsonObject>;
