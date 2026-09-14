//! Tool input examples appended to descriptions.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::JsonObject;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;

use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;

/// Formats one example (`example`, zero-based `index`) as a line.
pub type ExampleFormatFn = Arc<dyn Fn(&JsonObject, usize) -> String + Send + Sync>;

/// Middleware created by [`add_tool_input_examples`].
#[derive(Clone)]
pub struct AddToolInputExamples {
    prefix: String,
    format: Option<ExampleFormatFn>,
    remove: bool,
}

impl fmt::Debug for AddToolInputExamples {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddToolInputExamples")
            .field("prefix", &self.prefix)
            .field("custom_format", &self.format.is_some())
            .field("remove", &self.remove)
            .finish()
    }
}

/// Appends each function tool's `input_examples` to its description
/// (`"Input Examples:"` followed by one compact JSON line per example) and
/// removes them from the definition, for providers without an examples
/// field.
#[must_use]
pub fn add_tool_input_examples() -> AddToolInputExamples {
    AddToolInputExamples {
        prefix: "Input Examples:".to_owned(),
        format: None,
        remove: true,
    }
}

impl AddToolInputExamples {
    /// Heading placed before the examples.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Custom example formatter.
    #[must_use]
    pub fn format(
        mut self,
        format: impl Fn(&JsonObject, usize) -> String + Send + Sync + 'static,
    ) -> Self {
        self.format = Some(Arc::new(format));
        self
    }

    /// Whether the examples are removed from the tool definition after
    /// being appended (default `true`).
    #[must_use]
    pub fn remove(mut self, remove: bool) -> Self {
        self.remove = remove;
        self
    }

    fn format_example(&self, example: &JsonObject, index: usize) -> String {
        match &self.format {
            Some(format) => format(example, index),
            None => serde_json::to_string(example).unwrap_or_default(),
        }
    }

    /// Applies the transformation to `options`.
    #[must_use]
    pub fn apply(&self, mut options: CallOptions) -> CallOptions {
        for tool in &mut options.tools {
            if let ToolDefinition::Function {
                description,
                input_examples,
                ..
            } = tool
                && !input_examples.is_empty()
            {
                let formatted = input_examples
                    .iter()
                    .enumerate()
                    .map(|(index, example)| self.format_example(example, index))
                    .collect::<Vec<_>>()
                    .join("\n");
                let section = format!("{}\n{formatted}", self.prefix);
                *description = Some(match description.take() {
                    Some(existing) => format!("{existing}\n\n{section}"),
                    None => section,
                });
                if self.remove {
                    input_examples.clear();
                }
            }
        }
        options
    }
}

impl LanguageModelMiddleware for AddToolInputExamples {
    fn transform_params<'a>(
        &'a self,
        options: CallOptions,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        let options = self.apply(options);
        Box::pin(async move { Ok(options) })
    }
}
