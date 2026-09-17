//! Chat Completions API language model (`<name>.chat`).
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

pub mod api_types;
pub mod convert_prompt;
pub mod convert_tools;
pub mod options;
pub mod output;
pub mod stream;

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ToolCall;
use ferrin_spec::Warning;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use http::StatusCode;
use serde_json::json;

use self::api_types::ChatRequest;
use self::api_types::ChatResponse;
use self::convert_prompt::convert_prompt_for_provider;
use self::convert_tools::convert_tools;
use self::options::ChatProviderOptions;
use self::output::map_chat_finish_reason;
use self::output::map_chat_usage;
use self::output::prediction_metadata;
use self::stream::ChatStreamState;
use crate::capabilities::ModelCapabilities;
use crate::capabilities::SystemMessageMode;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::json_schema::normalize_json_schema;
use crate::responses::options::LogprobsOption;
use crate::responses::output::metadata;
use crate::stream_util::EarlyChunk;
use crate::stream_util::drive_stream;
use crate::stream_util::fail_on_early_error;
use crate::stream_util::timestamp_from_seconds;

/// A prepared Chat Completions request.
#[derive(Debug)]
pub struct PreparedChatRequest {
    /// Body.
    pub body: ChatRequest,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Language model backed by the Chat Completions API.
#[derive(Debug, Clone)]
pub struct OpenAiChatLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiChatLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("chat"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    fn call_options(&self, options: &CallOptions) -> Result<ChatProviderOptions, ProviderError> {
        let key = &self.config.provider_options_key;
        let parsed = parse_provider_options::<ChatProviderOptions>(key, &options.provider_options)?;
        if parsed.is_none() && key != "openai" {
            return Ok(
                parse_provider_options("openai", &options.provider_options)?.unwrap_or_default()
            );
        }
        Ok(parsed.unwrap_or_default())
    }

    /// Builds the request body.
    ///
    /// # Errors
    ///
    /// Returns conversion errors (invalid options, unsupported parts).
    pub fn prepare_request(
        &self,
        options: &CallOptions,
    ) -> Result<PreparedChatRequest, ProviderError> {
        let model_id = self.model_id.as_str();
        let caps = ModelCapabilities::for_model(model_id);
        let openai = self.call_options(options)?;
        let mut warnings = Vec::new();
        if options.top_k.is_some() {
            warnings.push(Warning::unsupported("topK"));
        }

        let mut reasoning_effort: Option<String> = openai.reasoning_effort.clone().or_else(|| {
            is_custom_reasoning(options.reasoning).then(|| options.reasoning.as_str().to_owned())
        });
        if let Some(effort) = &reasoning_effort
            && let Some(supported) = caps.supported_reasoning_efforts
            && !supported.contains(&effort.as_str())
        {
            warnings.push(Warning::unsupported_with_details(
                "reasoningEffort",
                format!(
                    "{model_id} only supports the following reasoning efforts: {}",
                    supported.join(", ")
                ),
            ));
            reasoning_effort = None;
        }
        let is_reasoning_model = openai.force_reasoning.unwrap_or(caps.is_reasoning_model);
        let system_message_mode = openai.system_message_mode.unwrap_or(if is_reasoning_model {
            SystemMessageMode::Developer
        } else {
            caps.system_message_mode
        });
        let messages = convert_prompt_for_provider(
            &options.prompt,
            system_message_mode,
            &self.config.provider_options_key,
            &self.config.name,
        )?;
        warnings.extend(messages.warnings);
        let strict_json_schema = openai.strict_json_schema.unwrap_or(true);
        let tools = convert_tools(
            &options.tools,
            options.tool_choice.as_ref(),
            strict_json_schema,
        )?;
        warnings.extend(tools.warnings);

        let response_format = match &options.response_format {
            Some(ResponseFormat::Json {
                schema: Some(schema),
                name,
                description,
            }) => {
                let (schema, schema_warnings) = normalize_json_schema(schema)?;
                warnings.extend(schema_warnings);
                let mut json_schema = json!({
                    "schema": schema,
                    "strict": strict_json_schema,
                    "name": name.clone().unwrap_or_else(|| "response".to_owned()),
                });
                if let Some(description) = description
                    && let Some(object) = json_schema.as_object_mut()
                {
                    object.insert(
                        "description".to_owned(),
                        JsonValue::from(description.as_str()),
                    );
                }
                Some(json!({"type": "json_schema", "json_schema": json_schema}))
            }
            Some(ResponseFormat::Json { .. }) => Some(json!({"type": "json_object"})),
            _ => None,
        };

        let (logprobs, top_logprobs) = match openai.logprobs {
            Some(LogprobsOption::Enabled(true)) => (Some(true), Some(0)),
            Some(LogprobsOption::Count(count)) => (Some(true), Some(count)),
            _ => (None, None),
        };

        let mut body = ChatRequest {
            model: model_id.to_owned(),
            messages: messages.messages,
            logit_bias: openai.logit_bias,
            logprobs,
            top_logprobs,
            user: openai.user,
            parallel_tool_calls: openai.parallel_tool_calls,
            max_tokens: options.max_output_tokens,
            temperature: options.temperature,
            top_p: options.top_p,
            frequency_penalty: options.frequency_penalty,
            presence_penalty: options.presence_penalty,
            response_format,
            stop: options.stop_sequences.clone().filter(|s| !s.is_empty()),
            seed: options.seed,
            verbosity: openai.text_verbosity,
            max_completion_tokens: openai.max_completion_tokens,
            store: openai.store,
            metadata: openai.metadata,
            prediction: openai.prediction,
            reasoning_effort,
            service_tier: openai.service_tier,
            prompt_cache_key: openai.prompt_cache_key,
            prompt_cache_options: openai.prompt_cache_options,
            prompt_cache_retention: openai.prompt_cache_retention,
            safety_identifier: openai.safety_identifier,
            tools: tools.tools,
            tool_choice: tools.tool_choice,
            stream: None,
            stream_options: None,
        };

        if is_reasoning_model {
            let sampling_allowed = body.reasoning_effort.as_deref() == Some("none")
                && caps.supports_non_reasoning_parameters;
            if !sampling_allowed {
                if body.temperature.take().is_some() {
                    warnings.push(Warning::unsupported_with_details(
                        "temperature",
                        "temperature is not supported for reasoning models",
                    ));
                }
                if body.top_p.take().is_some() {
                    warnings.push(Warning::unsupported_with_details(
                        "topP",
                        "topP is not supported for reasoning models",
                    ));
                }
                if body.logprobs.take().is_some() {
                    body.top_logprobs = None;
                    warnings.push(Warning::other(
                        "logprobs is not supported for reasoning models",
                    ));
                }
            }
            if body.frequency_penalty.take().is_some() {
                warnings.push(Warning::unsupported_with_details(
                    "frequencyPenalty",
                    "frequencyPenalty is not supported for reasoning models",
                ));
            }
            if body.presence_penalty.take().is_some() {
                warnings.push(Warning::unsupported_with_details(
                    "presencePenalty",
                    "presencePenalty is not supported for reasoning models",
                ));
            }
            if body.logit_bias.take().is_some() {
                warnings.push(Warning::other(
                    "logitBias is not supported for reasoning models",
                ));
            }
            if body.top_logprobs.take().is_some() {
                warnings.push(Warning::other(
                    "topLogprobs is not supported for reasoning models",
                ));
            }
            if let Some(max_tokens) = body.max_tokens.take()
                && body.max_completion_tokens.is_none()
            {
                body.max_completion_tokens = Some(max_tokens);
            }
        }
        if (model_id.starts_with("gpt-4o-search-preview")
            || model_id.starts_with("gpt-4o-mini-search-preview"))
            && body.temperature.take().is_some()
        {
            warnings.push(Warning::unsupported_with_details(
                "temperature",
                "temperature is not supported for the search preview models and has been removed.",
            ));
        }
        if body.service_tier.as_deref() == Some("flex") && !caps.supports_flex_processing {
            warnings.push(Warning::unsupported_with_details(
                "serviceTier",
                "flex processing is only available for o3, o4-mini, and gpt-5 models",
            ));
            body.service_tier = None;
        }
        if matches!(body.service_tier.as_deref(), Some("priority" | "fast"))
            && !caps.supports_priority_processing
        {
            warnings.push(Warning::unsupported_with_details(
                "serviceTier",
                "priority processing is only available for supported models (gpt-4, gpt-5, gpt-5-mini, o3, o4-mini) and requires Enterprise access. gpt-5-nano is not supported",
            ));
            body.service_tier = None;
        }
        Ok(PreparedChatRequest { body, warnings })
    }
}

fn classify_chunk(chunk: &ChatResponse) -> EarlyChunk {
    if chunk.error.is_some() {
        EarlyChunk::Error
    } else if chunk.has_output() {
        EarlyChunk::Output
    } else {
        EarlyChunk::Other
    }
}

impl LanguageModel for OpenAiChatLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        let http = || regex::Regex::new("^https?://.*$").into_iter();
        SupportedUrls::none().with("image/*", http())
    }

    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let url = self.config.url("/chat/completions");
        let handlers = ResponseHandlers::new(
            json_response_handler::<ChatResponse>(),
            failed_response_handler(),
        );
        let request_body = serde_json::to_value(&prepared.body).map_err(ProviderError::other)?;
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            self.config.headers(&options.headers)?,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let body = response.value;
        if let Some(error) = &body.error {
            let message = error
                .get("message")
                .and_then(JsonValue::as_str)
                .unwrap_or("OpenAI chat completion error");
            return Err(ApiCallError::new(message, url)
                .with_status(StatusCode::BAD_REQUEST)
                .with_request_body(request_body)
                .with_response(
                    response.response_headers,
                    response.raw.as_ref().map(ToString::to_string),
                )
                .retryable(false)
                .into());
        }
        let Some(choice) = body.choices.as_ref().and_then(|c| c.first()) else {
            return Err(InvalidResponseDataError::new(
                "response did not contain any choices",
                response.raw.unwrap_or(JsonValue::Null),
            )
            .into());
        };
        let mut content = Vec::new();
        let message = choice.message.clone().unwrap_or_default();
        if let Some(text) = message.content.filter(|text| !text.is_empty()) {
            content.push(Content::text(text));
        }
        for call in message.tool_calls.unwrap_or_default() {
            let id = call.id.unwrap_or_else(|| self.config.generate_id());
            let function = call.function.unwrap_or_default();
            content.push(Content::ToolCall(ToolCall::new(
                id,
                function.name.unwrap_or_default(),
                function.arguments.unwrap_or_else(|| "{}".to_owned()),
            )));
        }
        for annotation in message.annotations.unwrap_or_default() {
            if let Some(citation) = annotation.url_citation
                && annotation.kind == "url_citation"
            {
                content.push(Content::Source(Source::Url {
                    id: self.config.generate_id(),
                    url: citation.url,
                    title: citation.title,
                    provider_metadata: None,
                }));
            }
        }
        let mut provider_metadata = JsonObject::new();
        if let Some(usage) = &body.usage {
            for (key, count) in prediction_metadata(usage) {
                provider_metadata.insert(key, JsonValue::from(count));
            }
        }
        if let Some(logprobs) = choice.logprobs.as_ref().and_then(|l| l.content.clone()) {
            provider_metadata.insert("logprobs".to_owned(), JsonValue::Array(logprobs));
        }
        let finish_reason = choice.finish_reason.as_deref().map_or_else(
            || ferrin_spec::FinishReason::new(ferrin_spec::FinishReasonKind::Other),
            map_chat_finish_reason,
        );
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = body
            .usage
            .as_ref()
            .map(|usage| {
                map_chat_usage(
                    usage,
                    response
                        .raw
                        .as_ref()
                        .and_then(|raw| raw.get("usage"))
                        .and_then(JsonValue::as_object)
                        .cloned(),
                )
            })
            .unwrap_or_default();
        result.provider_metadata = Some(metadata(
            &self.config.provider_options_key,
            provider_metadata,
        ));
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata {
            id: body.id.clone(),
            timestamp: timestamp_from_seconds(body.created),
            model_id: body.model.clone().map(Into::into),
            headers: Some(response.response_headers),
            body: response.raw,
        };
        result.warnings = prepared.warnings;
        Ok(result)
    }

    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let mut prepared = self.prepare_request(&options)?;
        prepared.body.stream = Some(true);
        let mut stream_options = JsonObject::new();
        stream_options.insert("include_usage".to_owned(), JsonValue::Bool(true));
        prepared.body.stream_options = Some(stream_options);
        let url = self.config.url("/chat/completions");
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<ChatResponse>(),
            failed_response_handler(),
        );
        let request_body = serde_json::to_value(&prepared.body).map_err(ProviderError::other)?;
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            self.config.headers(&options.headers)?,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let chunks = fail_on_early_error(response.value, &url, classify_chunk).await?;
        let stream = drive_stream(
            StreamPart::StreamStart {
                warnings: prepared.warnings,
            },
            chunks,
            ChatStreamState::new(self.config.clone()),
            options.include_raw_chunks,
        );
        let mut result = StreamResult::new(stream);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata::with_headers(response.response_headers);
        Ok(result)
    }
}
