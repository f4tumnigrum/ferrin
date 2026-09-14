//! Responses API language model (`<name>.responses`).

pub mod api_types;
pub mod convert_prompt;
pub mod convert_tool_results;
pub mod convert_tools;
pub mod options;
pub mod output;
pub mod request;
pub mod stream;

use std::collections::HashMap;

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use http::StatusCode;

use self::api_types::ResponsesChunk;
use self::api_types::ResponsesResponse;
use self::output::OutputMapper;
use self::output::map_finish_reason;
use self::output::map_usage;
use self::request::prepare_request;
use self::stream::ResponsesStreamState;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::stream_util::EarlyChunk;
use crate::stream_util::drive_stream;
use crate::stream_util::fail_on_early_error;
use crate::stream_util::timestamp_from_seconds;

/// Language model backed by `POST /responses`.
#[derive(Debug, Clone)]
pub struct OpenAiResponsesLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiResponsesLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("responses"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }
}

/// Approval request id → tool call id, from tool calls in the prompt that
/// carry `approvalRequestId` in their provider options.
fn approval_ids_from_prompt(prompt: &[PromptMessage], key: &str) -> HashMap<String, String> {
    let mut mapping = HashMap::new();
    for message in prompt {
        let PromptMessage::Assistant { content, .. } = message else {
            continue;
        };
        for part in content {
            if let AssistantPromptPart::ToolCall(call) = part
                && let Some(approval) = call
                    .provider_options
                    .as_ref()
                    .and_then(|options| options.get(key))
                    .and_then(|options| options.get("approvalRequestId"))
                    .and_then(JsonValue::as_str)
            {
                mapping.insert(approval.to_owned(), call.tool_call_id.as_str().to_owned());
            }
        }
    }
    mapping
}

/// URLs the Responses API fetches itself: HTTP(S) images and PDFs.
#[must_use]
pub fn supported_urls() -> SupportedUrls {
    let http = || regex::Regex::new("^https?://.*$").into_iter();
    SupportedUrls::none()
        .with("image/*", http())
        .with("application/pdf", http())
}

fn classify_chunk(chunk: &ResponsesChunk) -> EarlyChunk {
    match chunk.kind.as_str() {
        "error" | "response.failed" => EarlyChunk::Error,
        "response.in_progress" => EarlyChunk::Accepted,
        "response.created" | "response.queued" => EarlyChunk::Other,
        _ => EarlyChunk::Output,
    }
}

impl LanguageModel for OpenAiResponsesLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        supported_urls()
    }

    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = prepare_request(&self.config, self.model_id.as_str(), &options)?;
        let url = self.config.url("/responses");
        let headers = self.config.headers(&options.headers)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<ResponsesResponse>(),
            failed_response_handler(),
        );
        let request_body = serde_json::to_value(&prepared.body).map_err(ProviderError::other)?;
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            headers,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let body = response.value;
        if let Some(error) = &body.error {
            return Err(ApiCallError::new(
                error
                    .message
                    .clone()
                    .unwrap_or_else(|| "OpenAI response error".to_owned()),
                url,
            )
            .with_status(StatusCode::BAD_REQUEST)
            .with_request_body(request_body)
            .with_response(
                response.response_headers,
                response.raw.as_ref().map(ToString::to_string),
            )
            .retryable(false)
            .into());
        }
        let Some(output) = &body.output else {
            let reason = body
                .incomplete_details
                .as_ref()
                .and_then(|d| d.reason.clone())
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(ApiCallError::new(
                format!("Responses API returned no output ({reason})"),
                url,
            )
            .with_status(StatusCode::INTERNAL_SERVER_ERROR)
            .with_request_body(request_body)
            .with_response(
                response.response_headers,
                response.raw.as_ref().map(ToString::to_string),
            )
            .retryable(false)
            .into());
        };
        let key = self.config.provider_options_key.clone();
        let collect_logprobs = options
            .provider_options
            .get(&key)
            .and_then(|o| o.get("logprobs"))
            .is_some();
        let mut mapper = OutputMapper::new(
            self.config.clone(),
            prepared.tool_name_mapping.clone(),
            prepared.web_search_tool_name.clone(),
        );
        mapper.approval_tool_call_ids = approval_ids_from_prompt(&options.prompt, &key);
        let mut content = Vec::new();
        for item in output {
            content.extend(mapper.map_item(item, collect_logprobs));
        }
        let incomplete = body
            .incomplete_details
            .as_ref()
            .and_then(|d| d.reason.as_deref());
        let finish_reason = map_finish_reason(incomplete, mapper.has_function_call);
        let raw_usage = response
            .raw
            .as_ref()
            .and_then(|raw| raw.get("usage"))
            .and_then(JsonValue::as_object)
            .cloned();
        let usage = body
            .usage
            .as_ref()
            .map(|usage| map_usage(usage, raw_usage))
            .unwrap_or_default();
        let provider_metadata = mapper.response_metadata(
            body.id.as_deref(),
            body.service_tier.as_deref(),
            body.reasoning.as_ref().and_then(|r| r.context.as_ref()),
        );
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = usage;
        result.provider_metadata = Some(provider_metadata);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata {
            id: body.id.clone(),
            timestamp: timestamp_from_seconds(body.created_at),
            model_id: body.model.clone().map(Into::into),
            headers: Some(response.response_headers),
            body: response.raw,
        };
        result.warnings = prepared.warnings;
        Ok(result)
    }

    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let mut prepared = prepare_request(&self.config, self.model_id.as_str(), &options)?;
        prepared.body.stream = Some(true);
        let url = self.config.url("/responses");
        let headers = self.config.headers(&options.headers)?;
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<ResponsesChunk>(),
            failed_response_handler(),
        );
        let request_body = serde_json::to_value(&prepared.body).map_err(ProviderError::other)?;
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            headers,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let chunks = fail_on_early_error(response.value, &url, classify_chunk).await?;

        let key = self.config.provider_options_key.clone();
        let collect_logprobs = options
            .provider_options
            .get(&key)
            .and_then(|o| o.get("logprobs"))
            .is_some();
        let mut mapper = OutputMapper::new(
            self.config.clone(),
            prepared.tool_name_mapping,
            prepared.web_search_tool_name,
        );
        mapper.approval_tool_call_ids = approval_ids_from_prompt(&options.prompt, &key);
        let state = ResponsesStreamState::new(mapper, prepared.store, collect_logprobs);
        let stream = drive_stream(
            StreamPart::StreamStart {
                warnings: prepared.warnings,
            },
            chunks,
            state,
            options.include_raw_chunks,
        );
        let mut result = StreamResult::new(stream);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata::with_headers(response.response_headers);
        Ok(result)
    }
}
