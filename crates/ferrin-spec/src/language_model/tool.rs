//! Tool definitions passed to language models.

use serde::Deserialize;
use serde::Serialize;

use crate::json::JsonObject;
use crate::json::JsonValue;
use crate::shared::ProviderOptions;
use crate::shared::ToolName;

/// A tool made available to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum ToolDefinition {
    /// A function tool executed by the application (or by the provider when
    /// it reports `provider_executed`).
    Function {
        /// Tool name.
        name: ToolName,
        /// Description shown to the model.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// JSON Schema (draft-07) of the input.
        input_schema: JsonValue,
        /// Whether the provider should enforce the schema strictly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
        /// Example inputs.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        input_examples: Vec<JsonObject>,
        /// Provider-specific options for this tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    /// A tool defined and executed by the provider (web search, code
    /// interpreter, ...).
    Provider {
        /// Tool id in `<provider>.<tool>` form.
        id: String,
        /// Name under which the tool appears in results.
        name: ToolName,
        /// Provider-specific arguments.
        #[serde(default)]
        args: JsonObject,
    },
}

impl ToolDefinition {
    /// Creates a function tool with a schema and optional description.
    #[must_use]
    pub fn function(
        name: impl Into<ToolName>,
        description: Option<String>,
        input_schema: JsonValue,
    ) -> Self {
        Self::Function {
            name: name.into(),
            description,
            input_schema,
            strict: None,
            input_examples: Vec::new(),
            provider_options: None,
        }
    }

    /// Creates a provider tool.
    #[must_use]
    pub fn provider(id: impl Into<String>, name: impl Into<ToolName>, args: JsonObject) -> Self {
        Self::Provider {
            id: id.into(),
            name: name.into(),
            args,
        }
    }

    /// Returns the tool name.
    #[must_use]
    pub fn name(&self) -> &ToolName {
        match self {
            Self::Function { name, .. } | Self::Provider { name, .. } => name,
        }
    }

    /// Returns `true` for [`ToolDefinition::Provider`].
    #[must_use]
    pub fn is_provider_tool(&self) -> bool {
        matches!(self, Self::Provider { .. })
    }
}
