//! Two-way renaming between application tool names and provider tool names.

use std::collections::HashMap;

use ferrin_spec::ToolDefinition;

/// Maps custom names of provider tools to the names the provider expects
/// and back.
///
/// Built from the tool definitions of a call: provider tools whose `id` is
/// in `provider_tool_names` are renamed; everything else passes through.
#[derive(Debug, Clone, Default)]
pub struct ToolNameMapping {
    to_provider: HashMap<String, String>,
    to_custom: HashMap<String, String>,
}

impl ToolNameMapping {
    /// Builds the mapping. `provider_tool_names` maps provider tool ids
    /// (`openai.web_search`) to the name used on the wire (`web_search`).
    #[must_use]
    pub fn new(tools: &[ToolDefinition], provider_tool_names: &HashMap<&str, &str>) -> Self {
        let mut mapping = Self::default();
        for tool in tools {
            if let ToolDefinition::Provider { id, name, .. } = tool
                && let Some(provider_name) = provider_tool_names.get(id.as_str())
            {
                mapping
                    .to_provider
                    .insert(name.as_str().to_owned(), (*provider_name).to_owned());
                mapping
                    .to_custom
                    .insert((*provider_name).to_owned(), name.as_str().to_owned());
            }
        }
        mapping
    }

    /// Adds an explicit pair.
    #[must_use]
    pub fn with_pair(mut self, custom: impl Into<String>, provider: impl Into<String>) -> Self {
        let custom = custom.into();
        let provider = provider.into();
        self.to_provider.insert(custom.clone(), provider.clone());
        self.to_custom.insert(provider, custom);
        self
    }

    /// Name to send to the provider.
    #[must_use]
    pub fn to_provider_tool_name<'a>(&'a self, custom: &'a str) -> &'a str {
        self.to_provider.get(custom).map_or(custom, String::as_str)
    }

    /// Name to report to the application.
    #[must_use]
    pub fn to_custom_tool_name<'a>(&'a self, provider: &'a str) -> &'a str {
        self.to_custom
            .get(provider)
            .map_or(provider, String::as_str)
    }

    /// Returns `true` when no renames are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.to_provider.is_empty()
    }
}
