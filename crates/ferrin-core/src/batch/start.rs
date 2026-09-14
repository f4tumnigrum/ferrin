//! `start_batch`.

use std::collections::HashMap;
use std::fmt;
use std::future::IntoFuture;
use std::sync::Arc;

use ferrin_spec::BatchRef;
use ferrin_spec::BoxFuture;
use ferrin_spec::ModelId;
use ferrin_spec::ToolDefinition;
use ferrin_spec::ToolName;
use ferrin_spec::batch::BatchRequest as ModelBatchRequest;
use ferrin_spec::batch::BatchStartOptions;
use ferrin_spec::batch::BatchStartResult;
use ferrin_spec::batch::ImageBatchRequestOptions;
use ferrin_spec::batch::TextBatchRequestOptions;
use tracing::Instrument;
use url::Url;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::prompt::DownloadFn;
use crate::prompt::convert::ConvertContext;
use crate::prompt::convert::convert_to_prompt;
use crate::prompt::prepare_tools::PrepareToolsInput;
use crate::prompt::prepare_tools::prepare_tools;
use crate::prompt::standardize::standardize;
use crate::telemetry::ModelIdentity;

use super::request::BatchRequest;
use super::request::validate_compatible_tools;
use super::request::validate_requests;
use super::service_identity;
use crate::telemetry::spans;

/// Starts a batch.
#[must_use]
pub fn start_batch(
    batch: impl Into<BatchRef>,
    requests: impl IntoIterator<Item = impl Into<BatchRequest>>,
) -> StartBatch {
    StartBatch {
        batch: batch.into(),
        requests: requests.into_iter().map(Into::into).collect(),
        webhook_url: None,
        download: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`start_batch`]; `.await` submits the batch.
pub struct StartBatch {
    batch: BatchRef,
    requests: Vec<BatchRequest>,
    webhook_url: Option<Url>,
    download: Option<Arc<dyn DownloadFn>>,
    base: ModalityOptions,
}

impl fmt::Debug for StartBatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartBatch")
            .field("batch", &self.batch)
            .field("requests", &self.requests.len())
            .field("webhook_url", &self.webhook_url)
            .field("has_download", &self.download.is_some())
            .field("base", &self.base)
            .finish()
    }
}

impl StartBatch {
    /// URL the provider calls when the batch completes.
    #[must_use]
    pub fn webhook_url(mut self, url: Url) -> Self {
        self.webhook_url = Some(url);
        self
    }

    /// Sets the function used to fetch prompt URLs the provider cannot.
    #[must_use]
    pub fn download(mut self, download: Arc<dyn DownloadFn>) -> Self {
        self.download = Some(download);
        self
    }
}

impl_modality_builder!(@no_retry StartBatch);

impl IntoFuture for StartBatch {
    type Output = Result<BatchStartResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run_start(self))
    }
}

async fn run_start(builder: StartBatch) -> Result<BatchStartResult, Error> {
    validate_requests(&builder.requests)?;
    let identity = service_identity(&builder.batch);
    let span = spans::modality_span("start_batch", &identity);
    let base = builder.base.clone();
    base.run(|base, token| {
        async move {
            let supported_urls = builder.batch.supported_urls().await;
            let mut normalized: Vec<ModelBatchRequest> = Vec::with_capacity(builder.requests.len());
            let mut model_ids: HashMap<String, ModelId> = HashMap::new();
            let mut seen_tools: HashMap<ToolName, ToolDefinition> = HashMap::new();
            for request in builder.requests {
                if token.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match request {
                    BatchRequest::Text(request) => {
                        let request = *request;
                        let standardized = standardize(
                            request.system,
                            request.prompt,
                            request.messages,
                            request.allow_system_in_messages,
                        )?;
                        request.settings.validate()?;
                        let prepared = prepare_tools(PrepareToolsInput {
                            tools: &request.tools,
                            active_tools: request.active_tools.as_deref(),
                            tool_order: &request.tool_order,
                            tool_choice: request.tool_choice,
                            tools_context: request.tools_context.as_ref(),
                            #[cfg(feature = "sandbox")]
                            sandbox: None,
                        })
                        .await?;
                        validate_compatible_tools(
                            &request.id,
                            &prepared.definitions,
                            &mut seen_tools,
                        )?;
                        let prompt = convert_to_prompt(
                            standardized.system.as_ref(),
                            &standardized.messages,
                            ConvertContext {
                                supported_urls: &supported_urls,
                                download: builder.download.as_deref(),
                                cancellation: &token,
                            },
                        )
                        .await?;
                        let settings = request.settings;
                        model_ids.insert(request.id.clone(), request.model_id.clone());
                        normalized.push(ModelBatchRequest::Text {
                            id: request.id,
                            model_id: request.model_id,
                            options: TextBatchRequestOptions {
                                prompt,
                                max_output_tokens: settings.max_output_tokens,
                                temperature: settings.temperature,
                                stop_sequences: settings.stop_sequences,
                                top_p: settings.top_p,
                                top_k: settings.top_k,
                                presence_penalty: settings.presence_penalty,
                                frequency_penalty: settings.frequency_penalty,
                                seed: settings.seed,
                                reasoning: settings.reasoning,
                                response_format: request.response_format,
                                tool_choice: prepared.tool_choice,
                                tools: prepared.definitions,
                                provider_options: settings.provider_options,
                            },
                        });
                    }
                    BatchRequest::Image(request) => {
                        let request = *request;
                        if request.n == 0 {
                            return Err(Error::invalid_argument("n", "must be at least 1"));
                        }
                        model_ids.insert(request.id.clone(), request.model_id.clone());
                        normalized.push(ModelBatchRequest::Image {
                            id: request.id,
                            model_id: request.model_id,
                            options: ImageBatchRequestOptions {
                                prompt: request.prompt,
                                n: request.n,
                                size: request.size,
                                aspect_ratio: request.aspect_ratio,
                                seed: request.seed,
                                files: request.files,
                                mask: request.mask,
                                provider_options: request.provider_options,
                            },
                        });
                    }
                    #[allow(unreachable_patterns, reason = "BatchRequest is non-exhaustive")]
                    _ => {
                        return Err(Error::invalid_argument(
                            "requests",
                            "unsupported batch request type",
                        ));
                    }
                }
            }
            let result = builder
                .batch
                .do_start_batch(BatchStartOptions {
                    requests: normalized,
                    webhook_url: builder.webhook_url,
                    provider_options: base.provider_options.clone(),
                    headers: base.request_headers(),
                    cancellation: token,
                })
                .await
                .map_err(Error::from)?;
            for warning in &result.warnings {
                let model_id = warning
                    .request_id
                    .as_ref()
                    .and_then(|id| model_ids.get(id))
                    .cloned()
                    .unwrap_or_else(|| ModelId::new("batch"));
                spans::log_warnings(
                    std::slice::from_ref(&warning.warning),
                    &ModelIdentity::new(identity.provider.clone(), model_id),
                );
            }
            Ok(result)
        }
        .instrument(span)
    })
    .await
}
