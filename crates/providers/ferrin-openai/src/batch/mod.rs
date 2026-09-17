//! Batch API (`<name>.batch`): text requests executed through the Responses
//! API.

pub mod api_types;
pub mod results;

use std::collections::VecDeque;

use bytes::Bytes;
use chrono::DateTime;
use chrono::Utc;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::batch::normalize_batch_request_counts;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::BatchId;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderId;
use ferrin_spec::Warning;
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
use ferrin_spec::batch::TextBatchRequestOptions;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use self::api_types::BatchProviderOptions;
use self::api_types::OpenAiBatchList;
use self::api_types::OpenAiBatchObject;
use self::results::ResultsState;
use self::results::results_stream;
use crate::config::SharedConfig;
use crate::embedding::provider_metadata;
use crate::error::failed_response_handler;
use crate::files::OpenAiFileObject;
use crate::responses::request::prepare_request;
use crate::stream_util::timestamp_from_seconds;

/// Endpoint executed for every batch line.
const BATCH_ENDPOINT: &str = "/v1/responses";
/// Default input file lifetime (48 hours).
const DEFAULT_INPUT_FILE_EXPIRES_AFTER: u64 = 172_800;
/// Smallest allowed input file lifetime (1 hour).
const MIN_INPUT_FILE_EXPIRES_AFTER: u64 = 3_600;
/// Largest allowed input file lifetime (30 days).
const MAX_INPUT_FILE_EXPIRES_AFTER: u64 = 2_592_000;
/// Provider tools whose output the result conversion understands.
const CONVERTIBLE_PROVIDER_TOOLS: &[&str] = &[
    "openai.programmatic_tool_calling",
    "openai.tool_search",
    "openai.shell",
    "openai.code_interpreter",
    "openai.custom",
    "openai.file_search",
    "openai.web_search",
    "openai.web_search_preview",
];
/// Output item types the result conversion understands.
const CONVERTIBLE_OUTPUT_TYPES: &[&str] = &[
    "program",
    "program_output",
    "tool_search_call",
    "tool_search_output",
    "shell_call",
    "shell_call_output",
    "reasoning",
    "message",
    "function_call",
    "custom_tool_call",
    "web_search_call",
    "file_search_call",
    "code_interpreter_call",
];

/// Maps a raw status.
#[must_use]
pub fn map_state(raw: &str) -> BatchState {
    match raw {
        "completed" => BatchState::Completed,
        "failed" | "expired" | "cancelled" => BatchState::Failed,
        _ => BatchState::Pending,
    }
}

fn unix_timestamp(value: Option<f64>) -> Option<DateTime<Utc>> {
    timestamp_from_seconds(value)
}

/// Converts a batch object to a status.
#[must_use]
pub fn map_status(batch: &OpenAiBatchObject) -> BatchStatus {
    let mut status = BatchStatus::new(map_state(&batch.status));
    status.raw_status = Some(batch.status.clone());
    if let Some(counts) = &batch.request_counts {
        let pending = match (counts.total, counts.completed, counts.failed) {
            (Some(total), Some(completed), Some(failed)) => {
                Some(total.saturating_sub(completed).saturating_sub(failed))
            }
            _ => None,
        };
        status.request_counts =
            normalize_batch_request_counts(counts.total, pending, counts.completed, counts.failed);
    }
    if let Some(first) = batch
        .errors
        .as_ref()
        .and_then(|errors| errors.data.as_ref())
        .and_then(|data| data.first())
    {
        status.error = Some(BatchError {
            message: first
                .message
                .clone()
                .unwrap_or_else(|| "OpenAI batch failed".to_owned()),
            error_type: None,
            code: first.code.clone(),
            status_code: None,
        });
    }
    status.created_at = unix_timestamp(batch.created_at);
    status.expires_at = unix_timestamp(batch.expires_at);
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

/// Batch service.
#[derive(Debug, Clone)]
pub struct OpenAiBatch {
    config: SharedConfig,
    provider: ProviderId,
}

impl OpenAiBatch {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("batch"),
            config,
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    async fn retrieve(
        &self,
        batch_id: &str,
        headers: &Headers,
        cancellation: CancellationToken,
    ) -> Result<OpenAiBatchObject, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiBatchObject>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(&format!("/batches/{batch_id}")),
            self.config.headers(headers)?,
            &handlers,
            cancellation,
        )
        .await?;
        Ok(response.value)
    }
}

impl Batch for OpenAiBatch {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn supported_urls(&self) -> SupportedUrls {
        crate::responses::supported_urls()
    }

    async fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> Result<BatchStartResult, ProviderError> {
        if options.requests.is_empty() {
            return Err(
                InvalidArgumentError::new("requests", "at least one request is required").into(),
            );
        }
        let mut model_id: Option<&str> = None;
        for request in &options.requests {
            let BatchRequest::Text {
                model_id: request_model,
                ..
            } = request
            else {
                return Err(UnsupportedFunctionalityError::new("batch request type: image").into());
            };
            match model_id {
                None => model_id = Some(request_model.as_str()),
                Some(first) if first != request_model.as_str() => {
                    return Err(InvalidArgumentError::new(
                        "requests",
                        format!(
                            "the OpenAI Batch API requires all requests in a batch to use the same model; found \"{first}\" and \"{request_model}\""
                        ),
                    )
                    .into());
                }
                Some(_) => {}
            }
        }
        let mut warnings = Vec::new();
        if options.webhook_url.is_some() {
            warnings.push(BatchWarning {
                request_id: None,
                warning: Warning::unsupported_with_details(
                    "webhookUrl",
                    "The OpenAI Batch API does not support per-batch webhook URLs.",
                ),
            });
        }
        let key = &self.config.provider_options_key;
        let mut batch_options =
            parse_provider_options::<BatchProviderOptions>(key, &options.provider_options)?;
        if batch_options.is_none() && key != "openai" {
            batch_options = parse_provider_options("openai", &options.provider_options)?;
        }
        let expires_after = batch_options
            .and_then(|o| o.input_file_expires_after)
            .unwrap_or(DEFAULT_INPUT_FILE_EXPIRES_AFTER);
        if !(MIN_INPUT_FILE_EXPIRES_AFTER..=MAX_INPUT_FILE_EXPIRES_AFTER).contains(&expires_after) {
            return Err(InvalidArgumentError::new(
                "inputFileExpiresAfter",
                format!(
                    "inputFileExpiresAfter must be between {MIN_INPUT_FILE_EXPIRES_AFTER} and {MAX_INPUT_FILE_EXPIRES_AFTER} seconds"
                ),
            )
            .into());
        }

        let mut lines: Vec<u8> = Vec::new();
        for request in &options.requests {
            let BatchRequest::Text {
                id,
                model_id,
                options: request_options,
            } = request
            else {
                continue;
            };
            let call = call_options(request_options);
            let prepared = prepare_request(&self.config, model_id.as_str(), &call)?;
            let line = json!({
                "custom_id": id,
                "method": "POST",
                "url": BATCH_ENDPOINT,
                "body": prepared.body,
            });
            serde_json::to_writer(&mut lines, &line).map_err(ProviderError::other)?;
            lines.push(b'\n');
            for warning in prepared.warnings {
                warnings.push(BatchWarning {
                    request_id: Some(id.clone()),
                    warning,
                });
            }
            for tool in &request_options.tools {
                if let ToolDefinition::Provider {
                    id: tool_id, name, ..
                } = tool
                    && !CONVERTIBLE_PROVIDER_TOOLS.contains(&tool_id.as_str())
                {
                    warnings.push(BatchWarning {
                        request_id: Some(id.clone()),
                        warning: Warning::unsupported_with_details(
                            format!("batch result conversion for tool \"{name}\""),
                            "OpenAI may return output for this tool that text batches cannot currently convert.",
                        ),
                    });
                }
            }
        }

        let headers = self.config.headers(&options.headers)?;
        let form = MultipartForm::new()
            .file(
                "file",
                Some("batch.jsonl".to_owned()),
                Some("application/jsonl".to_owned()),
                Bytes::from(lines),
            )
            .field("purpose", "batch")
            .field("expires_after[anchor]", "created_at")
            .field("expires_after[seconds]", expires_after.to_string());
        let upload_handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiFileObject>(),
            failed_response_handler(),
        );
        let uploaded = post_form(
            self.config.transport.as_ref(),
            self.config.url("/files"),
            headers.clone(),
            form,
            &upload_handlers,
            options.cancellation.clone(),
        )
        .await?
        .value;

        let batch_handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiBatchObject>(),
            failed_response_handler(),
        );
        let batch = post_json(
            self.config.transport.as_ref(),
            self.config.url("/batches"),
            headers,
            &json!({
                "input_file_id": uploaded.id,
                "endpoint": BATCH_ENDPOINT,
                "completion_window": "24h",
            }),
            &batch_handlers,
            options.cancellation,
        )
        .await?
        .value;

        let mut status = map_status(&batch);
        let mut metadata = JsonObject::new();
        metadata.insert("inputFileId".to_owned(), JsonValue::from(uploaded.id));
        if let Some(expires_at) = uploaded
            .expires_at
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
        {
            metadata.insert(
                "inputFileExpiresAt".to_owned(),
                JsonValue::from(expires_at.to_rfc3339()),
            );
        }
        status.provider_metadata = Some(provider_metadata(key, metadata));
        Ok(BatchStartResult {
            batch_id: BatchId::new(batch.id),
            status,
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
        let status = map_status(&batch);
        if status.status == BatchState::Pending {
            return Err(InvalidArgumentError::new(
                "batchId",
                format!("OpenAI batch \"{batch_id}\" is not complete"),
            )
            .into());
        }
        let file_ids: VecDeque<String> = [&batch.output_file_id, &batch.error_file_id]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        if status.status == BatchState::Completed && file_ids.is_empty() {
            return Err(InvalidResponseDataError::new(
                format!("OpenAI batch \"{batch_id}\" completed without batch output"),
                json!({"id": batch.id, "status": batch.status}),
            )
            .into());
        }
        Ok(results_stream(ResultsState {
            config: self.config.clone(),
            headers: self.config.headers(&options.headers)?,
            cancellation: options.cancellation,
            file_ids,
            current: None,
            done: false,
        }))
    }

    fn supports_cancel_batch(&self) -> bool {
        true
    }

    async fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> Result<BatchCancelResult, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiBatchObject>(),
            failed_response_handler(),
        );
        post_json(
            self.config.transport.as_ref(),
            self.config
                .url(&format!("/batches/{}/cancel", options.batch_id.as_str())),
            self.config.headers(&options.headers)?,
            &json!({}),
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(BatchCancelResult::default())
    }

    fn supports_list_batches(&self) -> bool {
        true
    }

    async fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> Result<BatchListResult, ProviderError> {
        let mut url = self.config.url("/batches");
        {
            let mut query = url.query_pairs_mut();
            if let Some(limit) = options.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(cursor) = &options.cursor {
                query.append_pair("after", cursor);
            }
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiBatchList>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?;
        let list = response.value;
        let next_cursor = list.has_more.then(|| list.last_id.clone()).flatten();
        Ok(BatchListResult {
            batches: list
                .data
                .iter()
                .map(|batch| BatchListItem {
                    batch_id: BatchId::new(batch.id.clone()),
                    status: map_status(batch),
                })
                .collect(),
            next_cursor,
            provider_metadata: None,
        })
    }
}
