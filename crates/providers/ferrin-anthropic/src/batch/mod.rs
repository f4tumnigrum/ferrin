//! Message Batches API (`<name>.batch`): text requests executed through the
//! Messages API.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

pub mod results;

use std::collections::BTreeSet;

use ferrin_provider_util::batch::normalize_batch_request_counts;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::BatchId;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use ferrin_spec::batch::Batch;
use ferrin_spec::batch::BatchCancelResult;
use ferrin_spec::batch::BatchListItem;
use ferrin_spec::batch::BatchListOptions;
use ferrin_spec::batch::BatchListResult;
use ferrin_spec::batch::BatchOperationOptions;
use ferrin_spec::batch::BatchRequest;
use ferrin_spec::batch::BatchResultStream;
use ferrin_spec::batch::BatchStartOptions;
use ferrin_spec::batch::BatchStartResult;
use ferrin_spec::batch::BatchState;
use ferrin_spec::batch::BatchStatus;
use ferrin_spec::batch::BatchWarning;
use ferrin_spec::batch::TextBatchRequestOptions;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::language_model::ToolDefinition;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use self::results::BatchResultLine;
use self::results::convert_line;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::files::parse_timestamp;
use crate::output::anthropic_metadata;
use crate::path::encode_path_segment;
use crate::request::prepare_request;

/// Request counts of a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AnthropicBatchRequestCounts {
    /// Requests still processing.
    #[serde(default)]
    pub processing: u64,
    /// Succeeded requests.
    #[serde(default)]
    pub succeeded: u64,
    /// Errored requests.
    #[serde(default)]
    pub errored: u64,
    /// Cancelled requests.
    #[serde(default)]
    pub canceled: u64,
    /// Expired requests.
    #[serde(default)]
    pub expired: u64,
}

/// A message batch object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AnthropicBatchObject {
    /// Batch id.
    pub id: String,
    /// `in_progress`, `canceling` or `ended`.
    pub processing_status: String,
    /// Request counts.
    #[serde(default)]
    pub request_counts: AnthropicBatchRequestCounts,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub created_at: Option<String>,
    /// Expiry time (RFC 3339).
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Archive time.
    #[serde(default)]
    pub archived_at: Option<String>,
    /// Cancellation start time.
    #[serde(default)]
    pub cancel_initiated_at: Option<String>,
    /// End time.
    #[serde(default)]
    pub ended_at: Option<String>,
    /// URL of the results file.
    #[serde(default)]
    pub results_url: Option<String>,
}

/// A page of batches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AnthropicBatchList {
    /// Batches.
    #[serde(default)]
    pub data: Vec<AnthropicBatchObject>,
    /// Whether more pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// Cursor of the next page.
    #[serde(default)]
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchProviderOptions {
    #[serde(default)]
    anthropic_beta: Option<Vec<String>>,
}

/// Whether `id` matches `^[A-Za-z0-9_-]{1,64}$`.
#[must_use]
pub fn is_valid_request_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Maps a processing status.
#[must_use]
pub fn map_state(raw: &str) -> BatchState {
    match raw {
        "ended" => BatchState::Completed,
        _ => BatchState::Pending,
    }
}

/// Converts a batch object to a status.
#[must_use]
pub fn map_status(batch: &AnthropicBatchObject) -> BatchStatus {
    let counts = &batch.request_counts;
    let mut status = BatchStatus::new(map_state(&batch.processing_status));
    status.raw_status = Some(batch.processing_status.clone());
    status.request_counts = normalize_batch_request_counts(
        Some(
            counts.processing
                + counts.succeeded
                + counts.errored
                + counts.canceled
                + counts.expired,
        ),
        Some(counts.processing),
        Some(counts.succeeded),
        Some(counts.errored + counts.canceled + counts.expired),
    );
    status.created_at = parse_timestamp(batch.created_at.as_deref());
    status.expires_at = parse_timestamp(batch.expires_at.as_deref());
    let mut meta = JsonObject::new();
    meta.insert(
        "archivedAt".to_owned(),
        batch
            .archived_at
            .clone()
            .map_or(JsonValue::Null, JsonValue::from),
    );
    meta.insert(
        "cancelInitiatedAt".to_owned(),
        batch
            .cancel_initiated_at
            .clone()
            .map_or(JsonValue::Null, JsonValue::from),
    );
    meta.insert(
        "endedAt".to_owned(),
        batch
            .ended_at
            .clone()
            .map_or(JsonValue::Null, JsonValue::from),
    );
    meta.insert(
        "requestCounts".to_owned(),
        json!({
            "processing": counts.processing,
            "succeeded": counts.succeeded,
            "errored": counts.errored,
            "canceled": counts.canceled,
            "expired": counts.expired,
        }),
    );
    meta.insert(
        "resultsUrl".to_owned(),
        batch
            .results_url
            .clone()
            .map_or(JsonValue::Null, JsonValue::from),
    );
    status.provider_metadata = Some(anthropic_metadata(meta));
    status
}

fn call_options(options: &TextBatchRequestOptions) -> CallOptions {
    let mut call = CallOptions::new(options.prompt.clone());
    call.max_output_tokens = options.max_output_tokens;
    call.temperature = options.temperature;
    call.stop_sequences = options.stop_sequences.clone();
    call.top_p = options.top_p;
    call.top_k = options.top_k;
    call.presence_penalty = options.presence_penalty;
    call.frequency_penalty = options.frequency_penalty;
    call.seed = options.seed;
    call.reasoning = options.reasoning;
    call.response_format = options.response_format.clone();
    call.tool_choice = options.tool_choice.clone();
    call.tools = options.tools.clone();
    call.provider_options = options.provider_options.clone();
    call
}

fn batch_betas(
    config: &SharedConfig,
    provider_options: &ProviderOptions,
) -> Result<BTreeSet<String>, ProviderError> {
    let mut betas = BTreeSet::new();
    let canonical =
        parse_provider_options::<BatchProviderOptions>(CANONICAL_OPTIONS_KEY, provider_options)?;
    let mut selected = canonical.and_then(|options| options.anthropic_beta);
    if config.options_key() != CANONICAL_OPTIONS_KEY
        && let Some(custom) =
            parse_provider_options::<BatchProviderOptions>(config.options_key(), provider_options)?
        && custom.anthropic_beta.is_some()
    {
        selected = custom.anthropic_beta;
    }
    betas.extend(selected.unwrap_or_default());
    Ok(betas)
}

fn unsupported(functionality: &str, message: String) -> ProviderError {
    UnsupportedFunctionalityError::with_message(functionality, message).into()
}

fn validate_body(body: &JsonObject, request_id: &str) -> Result<(), ProviderError> {
    if body.get("speed").is_some_and(|speed| !speed.is_null()) {
        return Err(unsupported(
            "providerOptions.anthropic.speed",
            format!("Anthropic Message Batches do not support speed (request \"{request_id}\")"),
        ));
    }
    if let Some(JsonValue::Array(fallbacks)) = body.get("fallbacks")
        && fallbacks
            .iter()
            .any(|fallback| fallback.get("speed").is_some_and(|speed| !speed.is_null()))
    {
        return Err(unsupported(
            "providerOptions.anthropic.fallbacks[].speed",
            format!(
                "Anthropic Message Batches do not support fallback speed (request \"{request_id}\")"
            ),
        ));
    }
    Ok(())
}

/// Batch service.
#[derive(Debug, Clone)]
pub struct AnthropicBatch {
    config: SharedConfig,
    provider: ProviderId,
}

impl AnthropicBatch {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("batch"),
            config,
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    fn batch_url(&self, batch_id: &str, suffix: &str) -> Url {
        self.config.url(&format!(
            "/messages/batches/{}{suffix}",
            encode_path_segment(batch_id)
        ))
    }

    async fn retrieve(
        &self,
        batch_id: &str,
        headers: &Headers,
        cancellation: CancellationToken,
    ) -> Result<AnthropicBatchObject, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicBatchObject>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.batch_url(batch_id, ""),
            self.config.headers(headers, &BTreeSet::new())?,
            &handlers,
            cancellation,
        )
        .await?;
        Ok(response.value)
    }

    fn same_origin(&self, url: &Url) -> bool {
        let base = &self.config.base_url;
        url.scheme() == base.scheme()
            && url.host_str() == base.host_str()
            && url.port_or_known_default() == base.port_or_known_default()
    }
}

impl Batch for AnthropicBatch {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn supported_urls(&self) -> SupportedUrls {
        crate::messages::supported_urls()
    }

    #[tracing::instrument(skip_all, fields(requests = options.requests.len()))]
    async fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> Result<BatchStartResult, ProviderError> {
        let mut seen = BTreeSet::new();
        for request in &options.requests {
            if !matches!(request, BatchRequest::Text { .. }) {
                return Err(unsupported(
                    "batch request type: image",
                    "The Anthropic Message Batches API does not support batch requests with type \"image\""
                        .to_owned(),
                ));
            }
            let id = request.id();
            if !is_valid_request_id(id) {
                return Err(InvalidArgumentError::new(
                    "requests",
                    format!(
                        "Anthropic batch request ID \"{id}\" must match ^[A-Za-z0-9_-]{{1,64}}$"
                    ),
                )
                .into());
            }
            if !seen.insert(id.to_owned()) {
                return Err(InvalidArgumentError::new(
                    "requests",
                    format!("Anthropic batch request IDs must be unique; duplicate ID \"{id}\""),
                )
                .into());
            }
        }
        let explicit_betas = batch_betas(&self.config, &options.provider_options)?;
        let mut betas = explicit_betas.clone();
        let mut warnings = Vec::new();
        if options.webhook_url.is_some() {
            warnings.push(BatchWarning {
                request_id: None,
                warning: Warning::unsupported_with_details(
                    "webhookUrl",
                    "The Anthropic Message Batches API does not support completion webhooks.",
                ),
            });
        }
        let mut requests = Vec::with_capacity(options.requests.len());
        for request in &options.requests {
            let BatchRequest::Text {
                id,
                model_id,
                options: request_options,
            } = request
            else {
                continue;
            };
            if !batch_betas(&self.config, &request_options.provider_options)?.is_empty() {
                return Err(unsupported(
                    "per-request providerOptions.anthropic.anthropicBeta",
                    format!(
                        "Anthropic Message Batches do not support per-request betas (request \"{id}\"). Set providerOptions.anthropic.anthropicBeta on startBatch instead"
                    ),
                ));
            }
            let call = call_options(request_options);
            let prepared = prepare_request(
                &self.config,
                model_id.as_str(),
                &call,
                false,
                explicit_betas.clone(),
            )?;
            if prepared.uses_json_response_tool {
                return Err(unsupported(
                    "batch responseFormat JSON-tool fallback",
                    format!(
                        "Anthropic Message Batches cannot decode the JSON-tool structured-output fallback (request \"{id}\") because batch results are retrieved independently of the start call. Use a model that supports native output_format structured outputs"
                    ),
                ));
            }
            if let Some(name) = request_options.tools.iter().find_map(|tool| match tool {
                ToolDefinition::Provider { name, .. }
                    if prepared
                        .tool_name_mapping
                        .to_provider_tool_name(name.as_str())
                        != name.as_str() =>
                {
                    Some(name.as_str().to_owned())
                }
                _ => None,
            }) {
                return Err(unsupported(
                    "aliased provider tool names in batches",
                    format!(
                        "Anthropic Message Batches cannot restore the custom provider-tool name \"{name}\" when results are retrieved independently of the start call (request \"{id}\"). Use the provider's canonical tool name"
                    ),
                ));
            }
            validate_body(&prepared.body, id)?;
            requests.push(json!({"custom_id": id, "params": prepared.body}));
            betas.extend(prepared.betas);
            warnings.extend(prepared.warnings.into_iter().map(|warning| BatchWarning {
                request_id: Some(id.clone()),
                warning,
            }));
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicBatchObject>(),
            failed_response_handler(),
        );
        let batch = post_json(
            self.config.transport.as_ref(),
            self.config.url("/messages/batches"),
            self.config.headers(&options.headers, &betas)?,
            &json!({"requests": requests}),
            &handlers,
            options.cancellation,
        )
        .await?
        .value;
        Ok(BatchStartResult {
            batch_id: BatchId::new(batch.id.clone()),
            status: map_status(&batch),
            warnings,
        })
    }

    async fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchStatus, ProviderError> {
        let batch = self
            .retrieve(
                options.batch_id.as_str(),
                &options.headers,
                options.cancellation,
            )
            .await?;
        Ok(map_status(&batch))
    }

    async fn do_get_batch_results(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchResultStream, ProviderError> {
        let batch_id = options.batch_id.as_str().to_owned();
        let batch = self
            .retrieve(&batch_id, &options.headers, options.cancellation.clone())
            .await?;
        if map_state(&batch.processing_status) == BatchState::Pending {
            return Err(InvalidArgumentError::new(
                "batchId",
                format!("Anthropic batch \"{batch_id}\" is not complete"),
            )
            .into());
        }
        if batch.archived_at.is_some() {
            return Err(InvalidArgumentError::new(
                "batchId",
                format!("Anthropic batch \"{batch_id}\" results are no longer available"),
            )
            .into());
        }
        let Some(results_url) = &batch.results_url else {
            return Err(InvalidResponseDataError::new(
                format!("Anthropic batch \"{batch_id}\" completed without batch output"),
                json!({"id": batch.id, "processing_status": batch.processing_status}),
            )
            .into());
        };
        let url = Url::parse(results_url).map_err(|_| {
            InvalidResponseDataError::new(
                format!("Anthropic batch \"{batch_id}\" has an invalid results URL"),
                json!({"id": batch.id}),
            )
        })?;
        let mut headers = self.config.headers(&options.headers, &BTreeSet::new())?;
        if !self.same_origin(&url) {
            headers.remove("x-api-key");
            headers.remove("authorization");
        }
        let handlers = ResponseHandlers::new(
            json_lines_response_handler::<BatchResultLine>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            url,
            headers,
            &handlers,
            options.cancellation,
        )
        .await?;
        let config = self.config.clone();
        let stream = response.value.map(move |parsed| match parsed {
            ferrin_provider_util::ParseResult::Ok { value, .. } => Ok(convert_line(&config, value)),
            ferrin_provider_util::ParseResult::Err { error, .. } => Err(error),
        });
        Ok(Box::pin(stream))
    }

    fn supports_cancel_batch(&self) -> bool {
        true
    }

    async fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchCancelResult, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicBatchObject>(),
            failed_response_handler(),
        );
        post_json(
            self.config.transport.as_ref(),
            self.batch_url(options.batch_id.as_str(), "/cancel"),
            self.config.headers(&options.headers, &BTreeSet::new())?,
            &json!({}),
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(BatchCancelResult {
            provider_metadata: None,
        })
    }

    fn supports_list_batches(&self) -> bool {
        true
    }

    async fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> Result<BatchListResult, ProviderError> {
        let mut url = self.config.url("/messages/batches");
        {
            let mut query = url.query_pairs_mut();
            if let Some(limit) = options.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(cursor) = &options.cursor {
                query.append_pair("after_id", cursor);
            }
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicBatchList>(),
            failed_response_handler(),
        );
        let page = get(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers, &BTreeSet::new())?,
            &handlers,
            options.cancellation,
        )
        .await?
        .value;
        Ok(BatchListResult {
            batches: page
                .data
                .iter()
                .map(|batch| BatchListItem {
                    batch_id: BatchId::new(batch.id.clone()),
                    status: map_status(batch),
                })
                .collect(),
            next_cursor: if page.has_more { page.last_id } else { None },
            provider_metadata: None,
        })
    }
}
