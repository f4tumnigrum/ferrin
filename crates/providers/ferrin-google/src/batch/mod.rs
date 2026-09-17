//! Batch generation (`batchGenerateContent`): inline or file-based input,
//! status, results, cancel and list.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

mod results;

use std::time::Duration;

use ferrin_provider_util::batch::normalize_batch_request_counts;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::http::text_response_handler;
use ferrin_spec::BatchId;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::batch::Batch;
use ferrin_spec::batch::BatchCancelResult;
use ferrin_spec::batch::BatchError;
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
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::shared::Warning;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::api_types::RpcStatus;
use crate::api_types::deserialize_count;
use crate::config::DOWNLOAD_PATH_PREFIX;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::files::GoogleFiles;
use crate::files::UploadRequest;
use crate::image::GoogleImageModel;
use crate::language_model::base_supported_urls;
use crate::output::OutputMapper;
use crate::request::prepare_request;

pub use self::results::BatchResultLine;
pub use self::results::convert_line;

/// Provider id family.
pub const FAMILY: &str = "batch";

/// Inline request bodies stay below this many bytes; larger batches are
/// uploaded as a JSONL file.
pub const INLINE_MAX_BYTES: usize = 20_000_000;

/// Maximum size of an uploaded JSONL input file.
pub const INPUT_FILE_MAX_BYTES: u64 = 2_000_000_000;

/// Prefix of the generated batch display names.
pub const DISPLAY_NAME_PREFIX: &str = "ferrin-batch-";

/// A long-running batch operation.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOperation {
    /// Operation name (`batches/...`), used as the batch id.
    pub name: String,
    /// Metadata (state, counts, output).
    #[serde(default)]
    pub metadata: Option<BatchOperationMetadata>,
    /// Whether the operation finished.
    #[serde(default)]
    pub done: Option<bool>,
    /// Error of a failed operation.
    #[serde(default)]
    pub error: Option<RpcStatus>,
    /// Response of a finished operation (same shape as `metadata.output`).
    #[serde(default)]
    pub response: Option<JsonValue>,
}

/// Metadata of a batch operation.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOperationMetadata {
    /// Raw state (`BATCH_STATE_PENDING`, `JOB_STATE_SUCCEEDED`, ...).
    #[serde(default)]
    pub state: Option<String>,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub create_time: Option<String>,
    /// Request counts.
    #[serde(default)]
    pub batch_stats: Option<BatchStats>,
    /// Output (`{responsesFile}` or `{inlinedResponses: {inlinedResponses}}`).
    #[serde(default)]
    pub output: Option<JsonValue>,
}

/// Request counts of a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStats {
    /// Total requests.
    #[serde(default, deserialize_with = "deserialize_count")]
    pub request_count: Option<u64>,
    /// Finished successfully.
    #[serde(default = "zero_count", deserialize_with = "deserialize_zero_count")]
    pub successful_request_count: Option<u64>,
    /// Failed.
    #[serde(default = "zero_count", deserialize_with = "deserialize_zero_count")]
    pub failed_request_count: Option<u64>,
    /// Not finished yet.
    #[serde(default = "zero_count", deserialize_with = "deserialize_zero_count")]
    pub pending_request_count: Option<u64>,
}

fn zero_count() -> Option<u64> {
    Some(0)
}

fn deserialize_zero_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let value = Option::<JsonValue>::deserialize(deserializer)?;
    Ok(match value {
        None => Some(0),
        Some(JsonValue::Number(value)) => value.as_u64(),
        Some(JsonValue::String(value))
            if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            value.parse().ok()
        }
        _ => None,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    #[serde(default)]
    operations: Vec<BatchOperation>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// Maps an RPC status to a batch error.
#[must_use]
pub fn rpc_error(status: &RpcStatus, fallback: &str) -> BatchError {
    BatchError {
        message: status
            .message
            .clone()
            .unwrap_or_else(|| fallback.to_owned()),
        error_type: status.status.clone(),
        code: status.code.map(|code| code.to_string()),
        status_code: None,
    }
}

/// Maps an operation to the normalized status.
#[must_use]
pub fn map_status(operation: &BatchOperation) -> BatchStatus {
    let metadata = operation.metadata.as_ref();
    let raw_state = metadata.and_then(|metadata| metadata.state.clone());
    let normalized = raw_state.as_deref().map(|state| {
        state
            .trim_start_matches("BATCH_STATE_")
            .trim_start_matches("JOB_STATE_")
    });
    let state = if operation.error.is_some() {
        BatchState::Failed
    } else {
        match normalized {
            Some("SUCCEEDED") => BatchState::Completed,
            Some("FAILED" | "CANCELLED" | "EXPIRED") => BatchState::Failed,
            Some(_) => BatchState::Pending,
            None if operation.done == Some(true) => BatchState::Completed,
            None => BatchState::Pending,
        }
    };
    let mut status = BatchStatus::new(state);
    status.raw_status = raw_state;
    status.error = operation
        .error
        .as_ref()
        .map(|error| rpc_error(error, "Google batch failed"));
    status.request_counts = metadata
        .and_then(|metadata| metadata.batch_stats.as_ref())
        .and_then(|stats| {
            normalize_batch_request_counts(
                stats.request_count,
                stats.pending_request_count,
                stats.successful_request_count,
                stats.failed_request_count,
            )
        });
    status.created_at = metadata
        .and_then(|metadata| metadata.create_time.as_deref())
        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        .map(|time| time.with_timezone(&chrono::Utc));
    status
}

fn batch_model_id(requests: &[BatchRequest]) -> Result<ModelId, ProviderError> {
    let mut model_id: Option<&ModelId> = None;
    for request in requests {
        let id = match request {
            BatchRequest::Text { model_id, .. } | BatchRequest::Image { model_id, .. } => model_id,
            #[allow(unreachable_patterns, reason = "BatchRequest is non-exhaustive")]
            _ => {
                return Err(ProviderError::unsupported(
                    "batch requests other than text and image",
                ));
            }
        };
        match model_id {
            None => model_id = Some(id),
            Some(first) if first != id => {
                return Err(InvalidArgumentError::new(
                    "requests",
                    "google batches require every request to use the same model because the model is part of the batch endpoint",
                )
                .into());
            }
            Some(_) => {}
        }
    }
    model_id.cloned().ok_or_else(|| {
        InvalidArgumentError::new("requests", "google batches require at least one request").into()
    })
}

/// A prepared batch request: the `generateContent` body and its warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedBatchRequest {
    /// Caller-assigned request id.
    pub id: String,
    /// Request body.
    pub body: JsonObject,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Batch service backed by `batchGenerateContent`.
#[derive(Debug, Clone)]
pub struct GoogleBatch {
    config: SharedConfig,
    provider: ProviderId,
}

impl GoogleBatch {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id(FAMILY),
            config,
        }
    }

    /// Builds the `generateContent` body of one batch request.
    ///
    /// # Errors
    ///
    /// Returns the language model's preparation errors; image requests with a
    /// mask or `n > 1` fail with [`ProviderError::InvalidArgument`].
    pub fn prepare_request(
        &self,
        request: &BatchRequest,
    ) -> Result<PreparedBatchRequest, ProviderError> {
        match request {
            BatchRequest::Text {
                id,
                model_id,
                options,
            } => {
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
                let prepared = prepare_request(&self.config, model_id.as_str(), &call)?;
                Ok(PreparedBatchRequest {
                    id: id.clone(),
                    body: prepared.body,
                    warnings: prepared.warnings,
                })
            }
            BatchRequest::Image {
                id,
                model_id,
                options,
            } => {
                let image_options = ImageOptions {
                    prompt: options.prompt.clone(),
                    n: options.n,
                    size: options.size,
                    aspect_ratio: options.aspect_ratio,
                    seed: options.seed,
                    files: options.files.clone(),
                    mask: options.mask.clone(),
                    provider_options: options.provider_options.clone(),
                    ..ImageOptions::default()
                };
                let (call, mut warnings) =
                    GoogleImageModel::new(self.config.clone(), model_id.clone())
                        .prepare_call(&image_options)?;
                let prepared = prepare_request(&self.config, model_id.as_str(), &call)?;
                warnings.extend(prepared.warnings);
                Ok(PreparedBatchRequest {
                    id: id.clone(),
                    body: prepared.body,
                    warnings,
                })
            }
            #[allow(unreachable_patterns, reason = "BatchRequest is non-exhaustive")]
            _ => Err(ProviderError::unsupported(
                "batch requests other than text and image",
            )),
        }
    }

    async fn retrieve(
        &self,
        options: &BatchOperationOptions,
    ) -> Result<(BatchOperation, Option<JsonValue>), ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<BatchOperation>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(options.batch_id.as_str()),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        Ok((response.value, response.raw))
    }

    fn line(prepared: &PreparedBatchRequest) -> String {
        json!({"key": prepared.id, "request": prepared.body}).to_string()
    }

    fn inlined(prepared: &PreparedBatchRequest) -> JsonValue {
        json!({"request": prepared.body, "metadata": {"key": prepared.id}})
    }

    #[allow(
        clippy::too_many_lines,
        reason = "inline/file input selection is one sequential flow"
    )]
    async fn build_start_body(
        &self,
        options: &BatchStartOptions,
        model_id: &ModelId,
        display_name: &str,
    ) -> Result<(JsonObject, Vec<BatchWarning>, Option<JsonObject>), ProviderError> {
        let mut warnings = Vec::new();
        let mut batch = JsonObject::new();
        batch.insert("displayName".to_owned(), JsonValue::from(display_name));
        if let Some(webhook) = &options.webhook_url {
            batch.insert(
                "webhookConfig".to_owned(),
                json!({"uris": [webhook.as_str()]}),
            );
        }
        let mut probe = batch.clone();
        probe.insert(
            "inputConfig".to_owned(),
            json!({"requests": {"requests": []}}),
        );
        let mut inline_bytes = json!({"batch": probe}).to_string().len();
        let mut inlined: Vec<PreparedBatchRequest> = Vec::new();
        let mut file_lines: Option<Vec<String>> = None;
        for request in &options.requests {
            let mut prepared = self.prepare_request(request)?;
            warnings.extend(prepared.warnings.drain(..).map(|warning| BatchWarning {
                request_id: Some(prepared.id.clone()),
                warning,
            }));
            if let Some(lines) = &mut file_lines {
                lines.push(Self::line(&prepared));
                continue;
            }
            let request_bytes = Self::inlined(&prepared).to_string().len();
            let next = inline_bytes + request_bytes + usize::from(!inlined.is_empty());
            if next < INLINE_MAX_BYTES {
                inlined.push(prepared);
                inline_bytes = next;
            } else {
                let mut lines: Vec<String> = inlined.iter().map(Self::line).collect();
                lines.push(Self::line(&prepared));
                inlined.clear();
                file_lines = Some(lines);
            }
        }
        let mut metadata = None;
        match file_lines {
            None => {
                let requests: Vec<JsonValue> = inlined.iter().map(Self::inlined).collect();
                batch.insert(
                    "inputConfig".to_owned(),
                    json!({"requests": {"requests": requests}}),
                );
            }
            Some(lines) => {
                let data = lines.join("\n");
                if data.len() as u64 > INPUT_FILE_MAX_BYTES {
                    return Err(InvalidArgumentError::new(
                        "requests",
                        "google batch input files must not exceed 2 GB",
                    )
                    .into());
                }
                let mut upload = UploadRequest::new(data.into(), "application/jsonl");
                upload.display_name = Some(format!("{display_name}-input"));
                upload.headers = options.headers.clone();
                upload.cancellation = options.cancellation.clone();
                upload.poll_interval =
                    Duration::from_millis(crate::files::DEFAULT_POLL_INTERVAL_MS);
                let file = GoogleFiles::new(self.config.clone())
                    .upload_bytes(upload)
                    .await?;
                batch.insert("inputConfig".to_owned(), json!({"fileName": file.name}));
                let mut object = JsonObject::new();
                object.insert("inputFileId".to_owned(), JsonValue::from(file.name));
                if let Some(expires) = file.expiration_time {
                    object.insert("inputFileExpiresAt".to_owned(), JsonValue::from(expires));
                }
                metadata = Some(object);
            }
        }
        let _ = model_id;
        Ok((batch, warnings, metadata))
    }
}

impl Batch for GoogleBatch {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn supported_urls(&self) -> SupportedUrls {
        base_supported_urls(&self.config.base_url)
    }

    #[tracing::instrument(skip_all, fields(requests = options.requests.len()))]
    async fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> Result<BatchStartResult, ProviderError> {
        let model_id = batch_model_id(&options.requests)?;
        let display_name = format!("{DISPLAY_NAME_PREFIX}{}", self.config.generate_id());
        let (batch, warnings, metadata) = self
            .build_start_body(&options, &model_id, &display_name)
            .await?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<BatchOperation>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config
                .model_url(model_id.as_str(), "batchGenerateContent"),
            self.config.headers(&options.headers)?,
            &json!({"batch": batch}),
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let operation = response.value;
        let mut status = map_status(&operation);
        if let Some(metadata) = metadata {
            let mapper = OutputMapper::new(self.config.clone(), Default::default());
            status.provider_metadata = Some(mapper.metadata(metadata));
        }
        Ok(BatchStartResult {
            batch_id: BatchId::new(operation.name),
            status,
            warnings,
        })
    }

    #[tracing::instrument(skip_all, fields(batch = %options.batch_id))]
    async fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchStatus, ProviderError> {
        let (operation, _) = self.retrieve(&options).await?;
        Ok(map_status(&operation))
    }

    #[tracing::instrument(skip_all, fields(batch = %options.batch_id))]
    async fn do_get_batch_results(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchResultStream, ProviderError> {
        let (operation, raw) = self.retrieve(&options).await?;
        let status = map_status(&operation);
        if status.status == BatchState::Pending {
            return Err(InvalidArgumentError::new(
                "batch_id",
                format!("google batch \"{}\" is not complete", options.batch_id),
            )
            .into());
        }
        let output = operation
            .metadata
            .and_then(|metadata| metadata.output)
            .or(operation.response)
            .unwrap_or(JsonValue::Null);
        if let Some(inlined) = output
            .get("inlinedResponses")
            .and_then(|value| value.get("inlinedResponses"))
            .and_then(JsonValue::as_array)
        {
            let config = self.config.clone();
            let items: Vec<Result<_, ProviderError>> = inlined
                .iter()
                .map(|item| {
                    let line = serde_json::from_value::<results::InlinedResponse>(item.clone())
                        .map_err(|error| {
                            ProviderError::InvalidResponseData(Box::new(
                                InvalidResponseDataError::new(
                                    format!(
                                        "google returned an invalid inlined batch response: {error}"
                                    ),
                                    item.clone(),
                                ),
                            ))
                        })?;
                    Ok(convert_line(&config, line.into_line()))
                })
                .collect();
            return Ok(Box::pin(futures_util::stream::iter(items)));
        }
        let Some(file) = output.get("responsesFile").and_then(JsonValue::as_str) else {
            if status.status == BatchState::Completed {
                return Err(ProviderError::InvalidResponseData(Box::new(
                    InvalidResponseDataError::new(
                        format!(
                            "google batch \"{}\" completed without batch output",
                            options.batch_id
                        ),
                        raw.unwrap_or(JsonValue::Null),
                    ),
                )));
            }
            return Ok(Box::pin(futures_util::stream::empty()));
        };
        let mut url = self
            .config
            .origin_url(&format!("{DOWNLOAD_PATH_PREFIX}{file}:download"));
        url.set_query(Some("alt=media"));
        let handlers = ResponseHandlers::new(
            json_lines_response_handler::<BatchResultLine>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let config = self.config.clone();
        Ok(Box::pin(response.value.map(move |parsed| {
            parsed.into_result().map(|line| convert_line(&config, line))
        })))
    }

    fn supports_cancel_batch(&self) -> bool {
        true
    }

    #[tracing::instrument(skip_all, fields(batch = %options.batch_id))]
    async fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchCancelResult, ProviderError> {
        let handlers = ResponseHandlers::new(text_response_handler(), failed_response_handler());
        post_json(
            self.config.transport.as_ref(),
            self.config
                .url(&format!("{}:cancel", options.batch_id.as_str())),
            self.config.headers(&options.headers)?,
            &json!({}),
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        Ok(BatchCancelResult::default())
    }

    fn supports_list_batches(&self) -> bool {
        true
    }

    #[tracing::instrument(skip_all)]
    async fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> Result<BatchListResult, ProviderError> {
        let mut url = self.config.url("batches");
        {
            let mut query = url.query_pairs_mut();
            if let Some(limit) = options.limit {
                query.append_pair("pageSize", &limit.to_string());
            }
            if let Some(cursor) = &options.cursor {
                query.append_pair("pageToken", cursor);
            }
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<ListResponse>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let batches = response
            .value
            .operations
            .iter()
            .map(|operation| BatchListItem {
                batch_id: BatchId::new(operation.name.clone()),
                status: map_status(operation),
            })
            .collect();
        Ok(BatchListResult {
            batches,
            next_cursor: response.value.next_page_token,
            provider_metadata: None,
        })
    }
}
