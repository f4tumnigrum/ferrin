//! Default embedding call settings.

use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::ProviderOptions;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::error::ProviderError;

use super::default_settings::merge_provider_options;
use crate::middleware::EmbeddingMiddlewareContext;
use crate::middleware::EmbeddingModelMiddleware;

/// Settings applied when the embedding call does not set them.
///
/// `headers` and `provider_options` are merged with the call's values taking
/// precedence (provider options merge recursively).
#[derive(Debug, Clone, Default)]
pub struct EmbeddingDefaults {
    /// See [`EmbedOptions::headers`].
    pub headers: Headers,
    /// See [`EmbedOptions::provider_options`].
    pub provider_options: ProviderOptions,
}

/// Middleware created by [`default_embedding_settings`].
#[derive(Debug, Clone)]
pub struct DefaultEmbeddingSettings {
    defaults: EmbeddingDefaults,
}

/// Fills unset embedding call options from `defaults`.
#[must_use]
pub fn default_embedding_settings(defaults: EmbeddingDefaults) -> DefaultEmbeddingSettings {
    DefaultEmbeddingSettings { defaults }
}

impl DefaultEmbeddingSettings {
    /// Applies the defaults to `options`.
    #[must_use]
    pub fn apply(&self, mut options: EmbedOptions) -> EmbedOptions {
        let defaults = &self.defaults;
        if !defaults.headers.is_empty() {
            let mut headers = defaults.headers.clone();
            headers.merge(&options.headers);
            options.headers = headers;
        }
        if !defaults.provider_options.is_empty() {
            options.provider_options =
                merge_provider_options(&defaults.provider_options, options.provider_options);
        }
        options
    }
}

impl EmbeddingModelMiddleware for DefaultEmbeddingSettings {
    fn transform_params<'a>(
        &'a self,
        options: EmbedOptions,
        _ctx: EmbeddingMiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<EmbedOptions, ProviderError>> {
        let options = self.apply(options);
        Box::pin(async move { Ok(options) })
    }
}
