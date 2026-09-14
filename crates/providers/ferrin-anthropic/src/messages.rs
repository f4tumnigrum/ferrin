//! Messages API language model (`<name>.messages`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::stream_driver::EarlyChunk;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_provider_util::stream_driver::fail_on_early_error;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;

use crate::api_types::AnthropicChunk;
use crate::api_types::AnthropicResponse;
use crate::config::SharedConfig;
use crate::error::FrameError;
use crate::error::failed_response_handler;
use crate::output::MessageMetadata;
use crate::output::OutputMapper;
use crate::output::citation_documents;
use crate::output::container_metadata;
use crate::output::web_tool_20260209_without_code_execution;
use crate::request::PreparedRequest;
use crate::request::prepare_request;
use crate::stream::AnthropicStreamState;
use crate::usage::convert_usage;
use crate::usage::map_stop_reason;

/// Language model backed by `POST /messages`.
#[derive(Debug, Clone)]
pub struct AnthropicMessagesLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

/// URLs the Messages API fetches itself: HTTP(S) images and PDFs.
#[must_use]
pub fn supported_urls() -> SupportedUrls {
    let http = || regex::Regex::new("^https?://.*$").into_iter();
    SupportedUrls::none()
        .with("image/*", http())
        .with("application/pdf", http())
}

fn classify_chunk(chunk: &AnthropicChunk) -> EarlyChunk {
    match chunk {
        AnthropicChunk::Error { .. } => EarlyChunk::Error,
        _ => EarlyChunk::Output,
    }
}

impl AnthropicMessagesLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("messages"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    fn mapper(&self, prepared: &PreparedRequest, options: &CallOptions) -> OutputMapper {
        let mut mapper = OutputMapper::new(self.config.clone(), prepared.tool_name_mapping.clone());
        mapper.uses_json_response_tool = prepared.uses_json_response_tool;
        mapper.mark_code_execution_dynamic =
            web_tool_20260209_without_code_execution(&options.tools);
        mapper.citation_documents = citation_documents(&self.config, &options.prompt);
        mapper
    }

    fn custom_key(&self, prepared: &PreparedRequest) -> Option<String> {
        prepared
            .used_custom_key
            .then(|| self.config.options_key().to_owned())
    }
}

impl LanguageModel for AnthropicMessagesLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        supported_urls()
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let user_betas = self.config.betas_from_headers(&options.headers);
        let prepared = prepare_request(
            &self.config,
            self.model_id.as_str(),
            &options,
            false,
            user_betas,
        )?;
        let url = self.config.url("/messages");
        let headers = self.config.headers(&options.headers, &prepared.betas)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicResponse>(),
            failed_response_handler(),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            headers,
            &request_body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let body = response.value;
        let mut mapper = self.mapper(&prepared, &options);
        let mut content = Vec::new();
        for block in &body.content {
            content.extend(mapper.map_block(block));
        }
        let finish_reason =
            map_stop_reason(body.stop_reason.as_deref(), mapper.json_response_from_tool);
        let raw_usage = response
            .raw
            .as_ref()
            .and_then(|raw| raw.get("usage"))
            .and_then(JsonValue::as_object)
            .cloned();
        let metadata = MessageMetadata {
            usage: raw_usage.clone(),
            stop_sequence: body.stop_sequence.clone(),
            stop_details: body.stop_details.as_ref(),
            input_transformations: body.input_transformations.as_ref(),
            iterations: body.usage.iterations.as_deref(),
            container: body
                .container
                .as_ref()
                .map(|container| container_metadata(container, true)),
            context_management: body.context_management.as_ref(),
        }
        .build(self.custom_key(&prepared).as_deref());
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = convert_usage(&body.usage, raw_usage);
        result.provider_metadata = Some(metadata);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata {
            id: body.id.clone(),
            timestamp: None,
            model_id: body.model.clone().map(Into::into),
            headers: Some(response.response_headers),
            body: response.raw,
        };
        result.warnings = prepared.warnings;
        Ok(result)
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let user_betas = self.config.betas_from_headers(&options.headers);
        let prepared = prepare_request(
            &self.config,
            self.model_id.as_str(),
            &options,
            true,
            user_betas,
        )?;
        let url = self.config.url("/messages");
        let headers = self.config.headers(&options.headers, &prepared.betas)?;
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<AnthropicChunk>(),
            failed_response_handler(),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            headers,
            &request_body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let early_body = request_body.clone();
        let chunks = fail_on_early_error(response.value, classify_chunk, move |chunk, raw| {
            let frame = match chunk {
                AnthropicChunk::Error { error } => JsonValue::Object(error.clone()),
                _ => raw.clone(),
            };
            FrameError::from_error_object(&frame)
                .to_api_call_error(url, &frame)
                .with_request_body(early_body)
                .into()
        })
        .await?;
        let mapper = self.mapper(&prepared, &options);
        let state = AnthropicStreamState::new(mapper, self.custom_key(&prepared));
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
