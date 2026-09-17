//! Voyage reranking options.
//!
//! Derived from Vercel AI SDK's Voyage reranking options schema
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); translated and modified.

use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::error::InvalidArgumentError;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Provider options under `voyage` or the configured provider name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoyageRerankingOptions {
    /// Whether the provider includes document text in the raw response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_documents: Option<bool>,
    /// Whether the provider truncates inputs exceeding its context length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<bool>,
}

pub(crate) fn parse_options(
    name: &str,
    options: &ProviderOptions,
) -> Result<VoyageRerankingOptions, InvalidArgumentError> {
    let mut merged = options.get("voyage").cloned().unwrap_or_default();
    if name != "voyage"
        && let Some(custom) = options.get(name)
    {
        merged.extend(custom.clone());
    }
    serde_json::from_value(JsonValue::Object(merged)).map_err(|_| {
        InvalidArgumentError::new(
            "provider_options",
            "voyage options must contain only optional returnDocuments and truncation booleans",
        )
    })
}
