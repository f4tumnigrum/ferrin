//! Gemini Interactions language models, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

mod background;
mod output;
mod prompt;
mod request;
mod sources;
mod stream;
mod stream_content;

use std::time::Duration;

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use tokio_util::sync::CancellationToken;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;

/// Headers and cancellation for an existing Interactions resource.
#[derive(Debug, Clone, Default)]
pub struct GoogleInteractionOptions {
    /// Extra request headers.
    pub headers: Headers,
    /// Cancellation of this resource operation.
    pub cancellation: CancellationToken,
}

/// A language model backed by Google's Interactions API.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> Result<(), ferrin_spec::error::ProviderError> {
/// use ferrin_google::{create_google, GoogleSettings};
/// use ferrin_spec::LanguageModel;
/// use ferrin_spec::language_model::{CallOptions, PromptMessage};
/// let google = create_google(GoogleSettings::default())?;
/// let model = google.interactions("gemini-2.5-flash");
/// let result = model.do_generate(CallOptions::new(vec![PromptMessage::user_text("Hello")])).await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct GoogleInteractionsLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl GoogleInteractionsLanguageModel {
    /// Creates an Interactions model using the shared provider configuration.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("interactions"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Converts a call to the Interactions request body without sending it.
    ///
    /// # Errors
    ///
    /// Returns an invalid argument error for unsupported options or file references.
    pub fn prepare_request(&self, options: &CallOptions) -> Result<JsonValue, ProviderError> {
        Ok(request::prepare(&self.config, self.model_id.as_str(), options)?.body)
    }

    /// Creates an interaction and returns its initial resource without polling.
    ///
    /// # Errors
    ///
    /// Returns option conversion, transport, provider or cancellation errors.
    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    pub async fn start_interaction(
        &self,
        options: CallOptions,
    ) -> Result<JsonValue, ProviderError> {
        let body = self.prepare_request(&options)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<JsonValue>(),
            failed_response_handler(),
        );
        Ok(post_json(
            self.config.transport.as_ref(),
            self.config.url("interactions"),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation,
        )
        .await?
        .value)
    }

    /// Retrieves an interaction resource by its server-assigned identifier.
    ///
    /// # Errors
    ///
    /// Returns an invalid argument error for an empty ID, or an HTTP/provider error.
    #[tracing::instrument(skip_all)]
    pub async fn get_interaction(
        &self,
        id: &str,
        options: GoogleInteractionOptions,
    ) -> Result<JsonValue, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<JsonValue>(),
            failed_response_handler(),
        );
        Ok(get(
            self.config.transport.as_ref(),
            background::url(&self.config, id)?,
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?
        .value)
    }

    /// Stops an interaction on the server, including a background run.
    ///
    /// # Errors
    ///
    /// Returns an invalid argument error for an empty ID, or an HTTP/provider error.
    #[tracing::instrument(skip_all)]
    pub async fn cancel_interaction(
        &self,
        id: &str,
        options: GoogleInteractionOptions,
    ) -> Result<JsonValue, ProviderError> {
        background::cancel(
            &self.config,
            id,
            self.config.headers(&options.headers)?,
            options.cancellation,
        )
        .await
    }

    async fn generate(
        &self,
        options: &CallOptions,
        prepared: &request::Prepared,
    ) -> Result<GenerateResult, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<JsonValue>(),
            failed_response_handler(),
        );
        let headers = self.config.headers(&options.headers)?;
        let mut response = post_json(
            self.config.transport.as_ref(),
            self.config.url("interactions"),
            headers.clone(),
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(prepared.timeout_ms);
        while matches!(response.value["status"].as_str(), Some("in_progress")) {
            if prepared.body["background"] != true && prepared.body.get("agent").is_none() {
                return Err(bad_response(
                    "nonterminal interactions response without background mode",
                ));
            }
            let id = response.value["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| bad_response("background interactions response omitted id"))?;
            let url = background::url(&self.config, id)?;
            let poll = async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                get(
                    self.config.transport.as_ref(),
                    url,
                    headers.clone(),
                    &handlers,
                    options.cancellation.clone(),
                )
                .await
            };
            response = tokio::select! {
                biased;
                () = options.cancellation.cancelled() => {
                    let _ = tokio::time::timeout(Duration::from_secs(2), background::cancel(&self.config, id, headers.clone(), CancellationToken::new())).await;
                    return Err(ProviderError::Cancelled);
                },
                response = tokio::time::timeout_at(deadline, poll) => response.map_err(|_| bad_response("background interactions polling timed out"))??,
            };
        }
        let mut result = output::convert(&self.config, &response.value, &prepared.aliases)?;
        result.warnings = prepared.warnings.clone();
        result.request = RequestMetadata::with_body(prepared.body.clone());
        result.response.headers = Some(response.response_headers);
        Ok(result)
    }
}

impl LanguageModel for GoogleInteractionsLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model_id
    }
    async fn supported_urls(&self) -> SupportedUrls {
        crate::language_model::supported_urls(&self.config.base_url, self.model_id.as_str())
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = request::prepare(&self.config, self.model_id.as_str(), &options)?;
        self.generate(&options, &prepared).await
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let mut prepared = request::prepare(&self.config, self.model_id.as_str(), &options)?;
        if prepared.body["background"] == true {
            let handlers = ResponseHandlers::new(
                json_response_handler::<JsonValue>(),
                failed_response_handler(),
            );
            let headers = self.config.headers(&options.headers)?;
            let response = post_json(
                self.config.transport.as_ref(),
                self.config.url("interactions"),
                headers.clone(),
                &prepared.body,
                &handlers,
                options.cancellation.clone(),
            )
            .await?;
            if response.value["status"] == "in_progress" {
                let id = response.value["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| bad_response("background interaction omitted id"))?;
                let chunks = background::stream(
                    self.config.clone(),
                    id.to_owned(),
                    headers,
                    options.cancellation,
                    prepared.timeout_ms,
                );
                let stream = drive_stream(
                    StreamPart::StreamStart {
                        warnings: prepared.warnings,
                    },
                    chunks,
                    stream::State::new(self.config.clone(), prepared.aliases)
                        .with_interaction(response.value),
                    options.include_raw_chunks,
                );
                let mut result = StreamResult::new(stream);
                result.request = RequestMetadata::with_body(prepared.body);
                result.response = ResponseMetadata::with_headers(response.response_headers);
                return Ok(result);
            }
            let mut result = output::convert(&self.config, &response.value, &prepared.aliases)?;
            result.warnings = prepared.warnings;
            result.request = RequestMetadata::with_body(prepared.body);
            result.response.headers = Some(response.response_headers);
            let mut parts = vec![StreamPart::StreamStart {
                warnings: result.warnings,
            }];
            if options.include_raw_chunks
                && let Some(raw) = result.response.body.clone()
            {
                parts.push(StreamPart::Raw { raw_value: raw });
            }
            parts.push(StreamPart::ResponseMetadata {
                id: result.response.id.clone(),
                timestamp: result.response.timestamp,
                model_id: result.response.model_id.clone(),
            });
            for (index, content) in result.content.into_iter().enumerate() {
                parts.extend(stream_content::content_parts(
                    content,
                    &format!("step-{index}").into(),
                ));
            }
            parts.push(StreamPart::Finish {
                finish_reason: result.finish_reason,
                usage: result.usage,
                provider_metadata: result.provider_metadata,
            });
            let mut stream = StreamResult::new(Box::pin(futures_util::stream::iter(parts)));
            stream.request = result.request;
            stream.response = result.response;
            return Ok(stream);
        }
        prepared.body["stream"] = JsonValue::Bool(true);
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<JsonValue>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("interactions"),
            self.config.headers(&options.headers)?,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let stream = drive_stream(
            StreamPart::StreamStart {
                warnings: prepared.warnings,
            },
            stream::terminal_chunks(response.value),
            stream::State::new(self.config.clone(), prepared.aliases),
            options.include_raw_chunks,
        );
        let mut result = StreamResult::new(stream);
        result.request = RequestMetadata::with_body(prepared.body);
        result.response = ResponseMetadata::with_headers(response.response_headers);
        Ok(result)
    }
}

fn invalid(argument: &str, message: &str) -> ProviderError {
    InvalidArgumentError::new(argument, message).into()
}

fn bad_response(message: &str) -> ProviderError {
    InvalidResponseDataError::new(message, JsonValue::Null).into()
}
