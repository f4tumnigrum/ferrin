//! Provider options of the chat model.

use serde::Deserialize;

/// Options read under the shared key and the provider name (unknown keys
/// under the provider name are passed through to the request body).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOptions {
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
    /// Reasoning effort for reasoning models.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Output verbosity.
    #[serde(default)]
    pub text_verbosity: Option<String>,
    /// Strict JSON schemas (default `true`).
    #[serde(default)]
    pub strict_json_schema: Option<bool>,
}

/// Option keys consumed by [`ChatOptions`]; every other key is passed
/// through.
pub const KNOWN_CHAT_OPTION_KEYS: &[&str] = &[
    "user",
    "reasoningEffort",
    "textVerbosity",
    "strictJsonSchema",
];
