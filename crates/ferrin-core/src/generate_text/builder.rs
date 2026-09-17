//! The `generate_text` builder.

use std::fmt;
use std::future::IntoFuture;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::LanguageModelRef;

use super::GenerateTextResult;
use super::config::CallConfig;
use super::run;
use crate::error::Error;
use crate::output::NoOutput;
use crate::output::Output;
use crate::output::OutputHandler;

/// Starts building a non-streaming text generation call.
///
/// The builder is a future: `.await` runs the generation loop.
#[must_use]
pub fn generate_text(model: impl Into<LanguageModelRef>) -> GenerateText<()> {
    GenerateText {
        config: CallConfig::new(model.into()),
        output: Arc::new(NoOutput),
    }
}

/// Builder and future of a `generate_text` call.
pub struct GenerateText<O> {
    pub(crate) config: CallConfig,
    pub(crate) output: Arc<dyn OutputHandler<O>>,
}

crate::builder::impl_call_builder!(GenerateText);

impl<O> GenerateText<O> {
    /// Requests structured output parsed by `output`.
    pub fn output<T>(self, output: Output<T>) -> GenerateText<T> {
        GenerateText {
            config: self.config,
            output: output.handler(),
        }
    }
}

impl<O> fmt::Debug for GenerateText<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerateText")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<O: Send + 'static> IntoFuture for GenerateText<O> {
    type Output = Result<GenerateTextResult<O>, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.output.validate_configuration()?;
            run::run(self.config, self.output).await
        })
    }
}
