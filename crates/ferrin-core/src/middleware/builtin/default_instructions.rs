//! Default system instructions.

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::prompt::PromptMessage;

use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;
use crate::prompt::Instructions;

/// Middleware created by [`default_instructions`].
#[derive(Debug, Clone)]
pub struct DefaultInstructions {
    instructions: Instructions,
}

/// Prepends `instructions` as system messages when the prompt has no
/// system message.
#[must_use]
pub fn default_instructions(instructions: impl Into<Instructions>) -> DefaultInstructions {
    DefaultInstructions {
        instructions: instructions.into(),
    }
}

impl LanguageModelMiddleware for DefaultInstructions {
    fn transform_params<'a>(
        &'a self,
        mut options: CallOptions,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        let has_system = options
            .prompt
            .iter()
            .any(|message| matches!(message, PromptMessage::System { .. }));
        if !has_system {
            options.prompt.splice(
                0..0,
                self.instructions
                    .as_messages()
                    .iter()
                    .map(|message| PromptMessage::System {
                        content: message.content.clone(),
                        provider_options: message.provider_options.clone(),
                    }),
            );
        }
        Box::pin(async move { Ok(options) })
    }
}
