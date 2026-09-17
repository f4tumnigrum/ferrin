//! Legacy Completions API language model (`<name>.completion`).

use std::collections::BTreeMap;

use ferrin_provider_util::ParseResult;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::PartId;
use ferrin_spec::ProviderId;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::InvalidPromptError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseFormat;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;

use crate::chat::output::map_chat_finish_reason;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::error::stream_error_for_frame;
use crate::responses::options::LogprobsOption;
use crate::responses::output::metadata;
use crate::stream_util::EarlyChunk;
use crate::stream_util::StreamMachine;
use crate::stream_util::drive_stream;
use crate::stream_util::fail_on_early_error;
use crate::stream_util::timestamp_from_seconds;

/// Id of the single text part.
const TEXT_PART_ID: &str = "0";

/// Call-level provider options.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionProviderOptions {
    /// Echo the prompt.
    #[serde(default)]
    pub echo: Option<bool>,
    /// Token biases.
    #[serde(default)]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    /// Log probabilities.
    #[serde(default)]
    pub logprobs: Option<LogprobsOption>,
    /// Suffix after the completion.
    #[serde(default)]
    pub suffix: Option<String>,
    /// End-user identifier.
    #[serde(default)]
    pub user: Option<String>,
}

/// Request body.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CompletionRequest {
    /// Model.
    pub model: String,
    /// Prompt text.
    pub prompt: String,
    /// Echo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo: Option<bool>,
    /// Token biases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    /// Number of log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<u32>,
    /// Suffix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// End-user identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Max tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Stream options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<JsonObject>,
}

/// Usage.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionUsage {
    /// Prompt tokens.
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens.
    #[serde(default)]
    pub completion_tokens: Option<u64>,
}

/// Choice.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionChoice {
    /// Text.
    #[serde(default)]
    pub text: Option<String>,
    /// Finish reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// Log probabilities.
    #[serde(default)]
    pub logprobs: Option<JsonValue>,
}

/// Response or chunk.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionResponse {
    /// Id.
    #[serde(default)]
    pub id: Option<String>,
    /// Created.
    #[serde(default)]
    pub created: Option<f64>,
    /// Model.
    #[serde(default)]
    pub model: Option<String>,
    /// Choices.
    #[serde(default)]
    pub choices: Option<Vec<CompletionChoice>>,
    /// Usage.
    #[serde(default)]
    pub usage: Option<CompletionUsage>,
    /// Error frame.
    #[serde(default)]
    pub error: Option<JsonValue>,
}

/// Converted prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPrompt {
    /// Prompt text.
    pub prompt: String,
    /// Stop sequences implied by the chat format.
    pub stop_sequences: Vec<String>,
}

/// Converts a prompt to the `user:` / `assistant:` text format.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidPrompt`] for a system message that is not
/// first and [`ProviderError::UnsupportedFunctionality`] for tool messages.
pub fn convert_prompt(prompt: &[PromptMessage]) -> Result<CompletionPrompt, ProviderError> {
    let mut text = String::new();
    let mut messages = prompt;
    if let [PromptMessage::System { content, .. }, rest @ ..] = messages {
        text.push_str(content);
        text.push_str("\n\n");
        messages = rest;
    }
    for message in messages {
        match message {
            PromptMessage::System { .. } => {
                return Err(InvalidPromptError::new("unexpected system message in prompt").into());
            }
            PromptMessage::User { content, .. } => {
                let user: String = content
                    .iter()
                    .filter_map(|part| match part {
                        UserPromptPart::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect();
                text.push_str("user:\n");
                text.push_str(&user);
                text.push_str("\n\n");
            }
            PromptMessage::Assistant { content, .. } => {
                let mut assistant = String::new();
                for part in content {
                    match part {
                        AssistantPromptPart::Text(t) => assistant.push_str(&t.text),
                        AssistantPromptPart::ToolCall(_) => {
                            return Err(
                                UnsupportedFunctionalityError::new("tool-call messages").into()
                            );
                        }
                        _ => {}
                    }
                }
                text.push_str("assistant:\n");
                text.push_str(&assistant);
                text.push_str("\n\n");
            }
            PromptMessage::Tool { .. } => {
                return Err(UnsupportedFunctionalityError::new("tool messages").into());
            }
            #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
            _ => {}
        }
    }
    text.push_str("assistant:\n");
    Ok(CompletionPrompt {
        prompt: text,
        stop_sequences: vec!["\nuser:".to_owned()],
    })
}

fn map_usage(usage: &CompletionUsage, raw: Option<JsonObject>) -> Usage {
    let mut result = Usage::default();
    result.input.total = usage.prompt_tokens;
    result.input.no_cache = Some(usage.prompt_tokens.unwrap_or(0));
    result.output.total = usage.completion_tokens;
    result.output.text = Some(usage.completion_tokens.unwrap_or(0));
    result.raw = raw;
    result
}

/// A prepared request.
#[derive(Debug)]
pub struct PreparedCompletionRequest {
    /// Body.
    pub body: CompletionRequest,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Language model backed by the legacy Completions API.
#[derive(Debug, Clone)]
pub struct OpenAiCompletionLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiCompletionLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("completion"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Builds the request body.
    ///
    /// # Errors
    ///
    /// Returns prompt conversion errors.
    pub fn prepare_request(
        &self,
        options: &CallOptions,
    ) -> Result<PreparedCompletionRequest, ProviderError> {
        let key = &self.config.provider_options_key;
        let mut openai =
            parse_provider_options::<CompletionProviderOptions>(key, &options.provider_options)?;
        if openai.is_none() && key != "openai" {
            openai = parse_provider_options("openai", &options.provider_options)?;
        }
        let openai = openai.unwrap_or_default();
        let mut warnings = Vec::new();
        if options.top_k.is_some() {
            warnings.push(Warning::unsupported("topK"));
        }
        if !options.tools.is_empty() {
            warnings.push(Warning::unsupported("tools"));
        }
        if options.tool_choice.is_some() {
            warnings.push(Warning::unsupported("toolChoice"));
        }
        if matches!(options.response_format, Some(ref format) if !matches!(format, ResponseFormat::Text))
        {
            warnings.push(Warning::unsupported_with_details(
                "responseFormat",
                "JSON response format is not supported.",
            ));
        }
        let converted = convert_prompt(&options.prompt)?;
        let mut stop = converted.stop_sequences;
        stop.extend(options.stop_sequences.clone().unwrap_or_default());
        let body = CompletionRequest {
            model: self.model_id.as_str().to_owned(),
            prompt: converted.prompt,
            echo: openai.echo,
            logit_bias: openai.logit_bias,
            logprobs: match openai.logprobs {
                Some(LogprobsOption::Enabled(true)) => Some(0),
                Some(LogprobsOption::Count(count)) => Some(count),
                _ => None,
            },
            suffix: openai.suffix,
            user: openai.user,
            max_tokens: options.max_output_tokens,
            temperature: options.temperature,
            top_p: options.top_p,
            frequency_penalty: options.frequency_penalty,
            presence_penalty: options.presence_penalty,
            seed: options.seed,
            stop: (!stop.is_empty()).then_some(stop),
            stream: None,
            stream_options: None,
        };
        Ok(PreparedCompletionRequest { body, warnings })
    }
}

fn classify_chunk(chunk: &CompletionResponse) -> EarlyChunk {
    if chunk.error.is_some() {
        EarlyChunk::Error
    } else if chunk.choices.as_ref().is_some_and(|choices| {
        choices
            .iter()
            .any(|c| c.text.as_ref().is_some_and(|t| !t.is_empty()))
    }) {
        EarlyChunk::Output
    } else {
        EarlyChunk::Other
    }
}

/// Stream state.
#[derive(Debug)]
struct CompletionStreamState {
    key: String,
    finish_reason: FinishReason,
    received_finish_reason: bool,
    usage: Option<(CompletionUsage, Option<JsonObject>)>,
    first_chunk: bool,
    provider_metadata: JsonObject,
}

impl StreamMachine for CompletionStreamState {
    type Chunk = CompletionResponse;

    fn handle(
        &mut self,
        chunk: ParseResult<CompletionResponse>,
        include_raw: bool,
    ) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        let (value, raw) = match chunk {
            ParseResult::Ok { value, raw } => (value, raw),
            ParseResult::Err { error, raw } => {
                if include_raw && let Some(raw) = raw {
                    parts.push(StreamPart::Raw {
                        raw_value: JsonValue::from(raw),
                    });
                }
                self.finish_reason = FinishReason::error();
                parts.push(StreamPart::error(&error));
                return parts;
            }
        };
        if include_raw {
            parts.push(StreamPart::Raw {
                raw_value: raw.clone(),
            });
        }
        if value.error.is_some() {
            self.finish_reason = FinishReason::error();
            parts.push(StreamPart::Error {
                error: stream_error_for_frame(&raw),
            });
            return parts;
        }
        if self.first_chunk {
            self.first_chunk = false;
            parts.push(StreamPart::ResponseMetadata {
                id: value.id.clone(),
                timestamp: timestamp_from_seconds(value.created),
                model_id: value.model.clone().map(Into::into),
            });
            parts.push(StreamPart::TextStart {
                id: PartId::new(TEXT_PART_ID),
                provider_metadata: None,
            });
        }
        if let Some(usage) = value.usage {
            self.usage = Some((
                usage,
                raw.get("usage").and_then(JsonValue::as_object).cloned(),
            ));
        }
        let Some(choice) = value
            .choices
            .and_then(|mut c| (!c.is_empty()).then(|| c.remove(0)))
        else {
            return parts;
        };
        if let Some(reason) = &choice.finish_reason {
            self.received_finish_reason = true;
            self.finish_reason = map_chat_finish_reason(reason);
        }
        if let Some(logprobs) = choice.logprobs {
            self.provider_metadata
                .insert("logprobs".to_owned(), logprobs);
        }
        if let Some(text) = choice.text.filter(|t| !t.is_empty()) {
            parts.push(StreamPart::TextDelta {
                id: PartId::new(TEXT_PART_ID),
                delta: text,
                provider_metadata: None,
            });
        }
        parts
    }

    fn finish(self) -> Vec<StreamPart> {
        if !self.received_finish_reason {
            return vec![StreamPart::error(&ProviderError::from(
                InvalidResponseDataError::new(
                    "completion stream ended before a finish reason was received",
                    JsonValue::Null,
                ),
            ))];
        }
        let mut parts = Vec::new();
        if !self.first_chunk {
            parts.push(StreamPart::TextEnd {
                id: PartId::new(TEXT_PART_ID),
                provider_metadata: None,
            });
        }
        parts.push(StreamPart::Finish {
            finish_reason: self.finish_reason,
            usage: self
                .usage
                .as_ref()
                .map(|(usage, raw)| map_usage(usage, raw.clone()))
                .unwrap_or_default(),
            provider_metadata: Some(metadata(&self.key, self.provider_metadata)),
        });
        parts
    }
}

impl LanguageModel for OpenAiCompletionLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        SupportedUrls::none()
    }

    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let url = self.config.url("/completions");
        let handlers = ResponseHandlers::new(
            json_response_handler::<CompletionResponse>(),
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
                .unwrap_or("OpenAI completion error");
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
        let mut provider_metadata = JsonObject::new();
        if let Some(logprobs) = &choice.logprobs {
            provider_metadata.insert("logprobs".to_owned(), logprobs.clone());
        }
        let content = vec![Content::text(choice.text.clone().unwrap_or_default())];
        let finish_reason = choice.finish_reason.as_deref().map_or_else(
            || FinishReason::new(FinishReasonKind::Other),
            map_chat_finish_reason,
        );
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = body
            .usage
            .as_ref()
            .map(|usage| {
                map_usage(
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
        let url = self.config.url("/completions");
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<CompletionResponse>(),
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
        let state = CompletionStreamState {
            key: self.config.provider_options_key.clone(),
            finish_reason: FinishReason::new(FinishReasonKind::Other),
            received_finish_reason: false,
            usage: None,
            first_chunk: true,
            provider_metadata: JsonObject::new(),
        };
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
