//! Application of middleware to a model.

use std::sync::Arc;

use ferrin_spec::CallOptions;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::GenerateResult;
use ferrin_spec::LanguageModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::StreamResult;
use ferrin_spec::SupportedUrls;
use ferrin_spec::error::ProviderError;

use super::CallKind;
use super::LanguageModelMiddleware;
use super::MiddlewareContext;
use super::tool_contract::ToolContract;

/// Wraps `model` with `middleware`; the first entry becomes the outermost
/// layer. An empty list returns `model` unchanged.
#[must_use]
pub fn wrap_language_model(
    model: Arc<dyn DynLanguageModel>,
    middleware: impl IntoIterator<
        Item = Arc<dyn LanguageModelMiddleware>,
        IntoIter: DoubleEndedIterator,
    >,
) -> Arc<dyn DynLanguageModel> {
    middleware.into_iter().rev().fold(model, |inner, layer| {
        let provider = layer
            .override_provider(inner.as_ref())
            .unwrap_or_else(|| inner.provider().clone());
        let model_id = layer
            .override_model_id(inner.as_ref())
            .unwrap_or_else(|| inner.model_id().clone());
        Arc::new(WrappedLanguageModel {
            inner,
            layer,
            provider,
            model_id,
        })
    })
}

struct WrappedLanguageModel {
    inner: Arc<dyn DynLanguageModel>,
    layer: Arc<dyn LanguageModelMiddleware>,
    provider: ProviderId,
    model_id: ModelId,
}

impl std::fmt::Debug for WrappedLanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrappedLanguageModel")
            .field("provider", &self.provider)
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl LanguageModel for WrappedLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        match self.layer.override_supported_urls(self.inner.as_ref()) {
            Some(urls) => urls.await,
            None => self.inner.supported_urls().await,
        }
    }

    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let ctx = MiddlewareContext {
            model: self.inner.as_ref(),
            kind: CallKind::Generate,
        };
        let contract = ToolContract::current();
        let options = self.layer.transform_params(options, ctx).await?;
        if let Some(contract) = &contract {
            contract.observe(&options);
        }
        let inner = &self.inner;
        self.layer
            .wrap_generate(
                options,
                Box::new(move |options| {
                    if let Some(contract) = &contract {
                        contract.observe(&options);
                    }
                    Box::pin(ToolContract::continue_call(
                        contract,
                        inner.do_generate(options),
                    ))
                }),
                ctx,
            )
            .await
    }

    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let ctx = MiddlewareContext {
            model: self.inner.as_ref(),
            kind: CallKind::Stream,
        };
        let contract = ToolContract::current();
        let options = self.layer.transform_params(options, ctx).await?;
        if let Some(contract) = &contract {
            contract.observe(&options);
        }
        let inner = &self.inner;
        self.layer
            .wrap_stream(
                options,
                Box::new(move |options| {
                    if let Some(contract) = &contract {
                        contract.observe(&options);
                    }
                    Box::pin(ToolContract::continue_call(
                        contract,
                        inner.do_stream(options),
                    ))
                }),
                ctx,
            )
            .await
    }
}
