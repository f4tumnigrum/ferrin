//! Chat Completions language model (`<name>.chat`).

pub mod api_types;
pub mod convert_prompt;
pub mod options;
pub mod output;
pub mod prepare_tools;
pub mod stream;

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_provider_util::stream_driver::EarlyChunk;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_provider_util::stream_driver::fail_on_early_error;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use serde_json::json;

use self::api_types::ChatResponse;
use self::convert_prompt::convert_prompt;
use self::options::ChatOptions;
use self::options::KNOWN_CHAT_OPTION_KEYS;
use self::output::convert_content;
use self::output::convert_usage;
use self::output::map_finish_reason;
use self::output::prediction_metadata;
use self::prepare_tools::prepare_tools;
use self::stream::ChatStreamState;
use crate::config::SharedConfig;
use crate::error::early_error;
use crate::error::failed_response_handler;
use crate::metadata::merge_metadata;
use crate::metadata::metadata_under;
use crate::metadata::timestamp_from_seconds;
use crate::options_key::DEPRECATED_SHARED_OPTIONS_KEY;
use crate::options_key::SHARED_OPTIONS_KEY;
use crate::options_key::merged_options;
use crate::options_key::option_keys;
use crate::options_key::passthrough_options;
use crate::options_key::resolve_metadata_key;
use crate::options_key::warn_if_deprecated_key;

/// A prepared request.
#[derive(Debug, Clone)]
pub struct PreparedChatRequest {
    /// Body (before the request body transformer).
    pub body: JsonObject,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// Key under which provider metadata is written.
    pub metadata_key: String,
}

/// Language model backed by `POST /chat/completions`.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleChatLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

fn insert_some(body: &mut JsonObject, key: &str, value: Option<impl Into<JsonValue>>) {
    if let Some(value) = value {
        body.insert(key.to_owned(), value.into());
    }
}

impl OpenAiCompatibleChatLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("chat"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Builds the request body.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidArgument`] for malformed provider
    /// options and prompt conversion errors.
    #[allow(
        clippy::too_many_lines,
        reason = "one field per line of the request body"
    )]
    pub fn prepare_request(
        &self,
        options: &CallOptions,
    ) -> Result<PreparedChatRequest, ProviderError> {
        let name = &self.config.name;
        let mut warnings = Vec::new();
        if options
            .provider_options
            .contains_key(DEPRECATED_SHARED_OPTIONS_KEY)
        {
            warnings.push(Warning::deprecated(
                format!("providerOptions key '{DEPRECATED_SHARED_OPTIONS_KEY}'"),
                format!("Use '{SHARED_OPTIONS_KEY}' instead."),
            ));
        }
        warn_if_deprecated_key(name, &options.provider_options, &mut warnings);
        let compatible: ChatOptions =
            match merged_options(&option_keys(name), &options.provider_options) {
                Some(object) => {
                    serde_json::from_value(JsonValue::Object(object)).map_err(|error| {
                        InvalidArgumentError::new(
                            "provider_options",
                            format!("invalid {name} provider options: {error}"),
                        )
                    })?
                }
                None => ChatOptions::default(),
            };
        let strict_json_schema = compatible.strict_json_schema.unwrap_or(true);
        if options.top_k.is_some() {
            warnings.push(Warning::unsupported("topK"));
        }
        if let Some(ResponseFormat::Json {
            schema: Some(_), ..
        }) = &options.response_format
            && !self.config.supports_structured_outputs
        {
            warnings.push(Warning::unsupported_with_details(
                "responseFormat",
                "JSON response format schema is only supported with structuredOutputs",
            ));
        }
        let tools = prepare_tools(&options.tools, options.tool_choice.as_ref())?;
        warnings.extend(tools.warnings);
        let metadata_key = resolve_metadata_key(name, &options.provider_options);
        let messages = convert_prompt(&options.prompt, &metadata_key)?;
        warnings.extend(messages.warnings);

        let mut body = JsonObject::new();
        body.insert("model".to_owned(), JsonValue::from(self.model_id.as_str()));
        insert_some(&mut body, "user", compatible.user.clone());
        insert_some(&mut body, "max_tokens", options.max_output_tokens);
        insert_some(&mut body, "temperature", options.temperature);
        insert_some(&mut body, "top_p", options.top_p);
        insert_some(&mut body, "frequency_penalty", options.frequency_penalty);
        insert_some(&mut body, "presence_penalty", options.presence_penalty);
        match &options.response_format {
            Some(ResponseFormat::Json {
                schema: Some(schema),
                name: schema_name,
                description,
            }) if self.config.supports_structured_outputs => {
                let mut json_schema = json!({
                    "schema": schema,
                    "strict": strict_json_schema,
                    "name": schema_name.clone().unwrap_or_else(|| "response".to_owned()),
                });
                if let Some(description) = description
                    && let Some(object) = json_schema.as_object_mut()
                {
                    object.insert(
                        "description".to_owned(),
                        JsonValue::from(description.as_str()),
                    );
                }
                body.insert(
                    "response_format".to_owned(),
                    json!({"type": "json_schema", "json_schema": json_schema}),
                );
            }
            Some(ResponseFormat::Json { .. }) => {
                body.insert("response_format".to_owned(), json!({"type": "json_object"}));
            }
            _ => {}
        }
        insert_some(
            &mut body,
            "stop",
            options.stop_sequences.clone().filter(|s| !s.is_empty()),
        );
        insert_some(&mut body, "seed", options.seed);
        body.extend(passthrough_options(
            name,
            &options.provider_options,
            KNOWN_CHAT_OPTION_KEYS,
        ));
        let reasoning_effort = compatible.reasoning_effort.clone().or_else(|| {
            is_custom_reasoning(options.reasoning).then(|| options.reasoning.as_str().to_owned())
        });
        insert_some(&mut body, "reasoning_effort", reasoning_effort);
        insert_some(&mut body, "verbosity", compatible.text_verbosity);
        body.insert("messages".to_owned(), JsonValue::Array(messages.messages));
        insert_some(&mut body, "tools", tools.tools.map(JsonValue::Array));
        insert_some(&mut body, "tool_choice", tools.tool_choice);
        Ok(PreparedChatRequest {
            body,
            warnings,
            metadata_key,
        })
    }

    fn handlers<T>(
        &self,
        success: impl ferrin_provider_util::http::ResponseHandler<T> + 'static,
    ) -> ResponseHandlers<T> {
        ResponseHandlers::new(
            success,
            failed_response_handler(self.config.error_structure.clone()),
        )
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

impl LanguageModel for OpenAiCompatibleChatLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        self.config.supported_urls.clone()
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let body = self.config.transform_request_body(prepared.body);
        let url = self.config.url("/chat/completions");
        let handlers = self.handlers(json_response_handler::<ChatResponse>());
        let request_body = JsonValue::Object(body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let value = response.value;
        let raw = response.raw.unwrap_or(JsonValue::Null);
        let Some(choice) = value.choices.as_ref().and_then(|c| c.first()) else {
            return Err(
                InvalidResponseDataError::new("response did not contain any choices", raw).into(),
            );
        };
        let message = choice.message.clone().unwrap_or_default();
        let mut content = convert_content(message.content.as_ref());
        if let Some(reasoning) = message.reasoning_text().filter(|text| !text.is_empty()) {
            content.push(Content::reasoning(reasoning));
        }
        for call in message.tool_calls.unwrap_or_default() {
            let id = call
                .id
                .clone()
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| self.config.generate_id());
            let function = call.function.clone().unwrap_or_default();
            let mut tool_call = ToolCall::new(
                id,
                function.name.unwrap_or_default(),
                function.arguments.unwrap_or_default(),
            );
            if let Some(signature) = call.thought_signature() {
                let mut object = JsonObject::new();
                object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
                tool_call.provider_metadata = Some(metadata_under(&prepared.metadata_key, object));
            }
            content.push(Content::ToolCall(tool_call));
        }
        let mut provider_metadata = metadata_under(
            &prepared.metadata_key,
            value
                .usage
                .as_ref()
                .map(prediction_metadata)
                .unwrap_or_default(),
        );
        if let Some(extractor) = &self.config.metadata_extractor
            && let Some(extra) = extractor.extract_metadata(&raw)
        {
            merge_metadata(&mut provider_metadata, extra);
        }
        let finish_reason = choice.finish_reason.as_deref().map_or_else(
            || FinishReason::new(FinishReasonKind::Other),
            map_finish_reason,
        );
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = match &value.usage {
            Some(usage) => match &self.config.convert_usage {
                Some(convert) => convert(usage),
                None => convert_usage(
                    usage,
                    raw.get("usage").and_then(JsonValue::as_object).cloned(),
                ),
            },
            None => Usage::default(),
        };
        result.provider_metadata = Some(provider_metadata);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata {
            id: value.id.clone(),
            timestamp: timestamp_from_seconds(value.created),
            model_id: value.model.clone().map(Into::into),
            headers: Some(response.response_headers),
            body: Some(raw),
        };
        result.warnings = prepared.warnings;
        Ok(result)
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let mut body = prepared.body;
        body.insert("stream".to_owned(), JsonValue::Bool(true));
        if self.config.include_usage {
            body.insert("stream_options".to_owned(), json!({"include_usage": true}));
        }
        let body = self.config.transform_request_body(body);
        let url = self.config.url("/chat/completions");
        let handlers = self.handlers(event_source_response_handler::<ChatResponse>());
        let request_body = JsonValue::Object(body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let structure = self.config.error_structure.clone();
        let chunks = fail_on_early_error(response.value, classify_chunk, move |_, raw| {
            early_error(structure.as_ref(), url, raw)
                .with_request_body(request_body.clone())
                .into()
        })
        .await?;
        let extractor = self
            .config
            .metadata_extractor
            .as_ref()
            .map(|extractor| extractor.stream_extractor());
        let stream = drive_stream(
            StreamPart::StreamStart {
                warnings: prepared.warnings,
            },
            chunks,
            ChatStreamState::new(self.config.clone(), prepared.metadata_key, extractor),
            options.include_raw_chunks,
        );
        let mut result = StreamResult::new(stream);
        result.request = RequestMetadata::with_body(JsonValue::Object(body));
        result.response = ResponseMetadata::with_headers(response.response_headers);
        Ok(result)
    }
}
