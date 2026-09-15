//! Legacy Completions language model (`<name>.completion`).

use std::collections::BTreeMap;

use ferrin_provider_util::ParseResult;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::stream_driver::EarlyChunk;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_provider_util::stream_driver::fail_on_early_error;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::PartId;
use ferrin_spec::ProviderId;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
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
use serde::Deserialize;
use serde_json::json;

use crate::chat::output::map_finish_reason;
use crate::config::SharedConfig;
use crate::error::early_error;
use crate::error::failed_response_handler;
use crate::error::stream_error_for_frame;
use crate::metadata::timestamp_from_seconds;
use crate::options_key::merged_options;
use crate::options_key::option_keys;
use crate::options_key::passthrough_options;
use crate::options_key::warn_if_deprecated_key;

/// Id of the single text part.
const TEXT_PART_ID: &str = "0";

/// Provider options of the completion model (unknown keys under the
/// provider name are passed through to the request body).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOptions {
    /// Echo the prompt.
    #[serde(default)]
    pub echo: Option<bool>,
    /// Token id → bias.
    #[serde(default)]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    /// Suffix after the completion.
    #[serde(default)]
    pub suffix: Option<String>,
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
}

/// Option keys consumed by [`CompletionOptions`].
pub const KNOWN_COMPLETION_OPTION_KEYS: &[&str] = &["echo", "logitBias", "suffix", "user"];

/// Usage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct CompletionUsage {
    /// Prompt tokens.
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens.
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// Total tokens.
    #[serde(default)]
    pub total_tokens: Option<u64>,
}

/// Choice.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CompletionChoice {
    /// Text.
    #[serde(default)]
    pub text: Option<String>,
    /// Finish reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Response or chunk.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CompletionResponse {
    /// Id.
    #[serde(default)]
    pub id: Option<String>,
    /// Created (seconds).
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
/// first and [`ProviderError::UnsupportedFunctionality`] for tool calls and
/// tool messages.
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

/// Maps usage: input and output totals, no cache or reasoning breakdown.
#[must_use]
pub fn convert_completion_usage(usage: &CompletionUsage, raw: Option<JsonObject>) -> Usage {
    let input = usage.prompt_tokens.unwrap_or(0);
    let output = usage.completion_tokens.unwrap_or(0);
    let mut result = Usage::totals(input, output);
    result.input.no_cache = Some(input);
    result.output.text = Some(output);
    result.raw = raw;
    result
}

/// A prepared request.
#[derive(Debug, Clone)]
pub struct PreparedCompletionRequest {
    /// Body.
    pub body: JsonObject,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Language model backed by `POST /completions`.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleCompletionLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

fn insert_some(body: &mut JsonObject, key: &str, value: Option<impl Into<JsonValue>>) {
    if let Some(value) = value {
        body.insert(key.to_owned(), value.into());
    }
}

impl OpenAiCompatibleCompletionLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("completion"),
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
    pub fn prepare_request(
        &self,
        options: &CallOptions,
    ) -> Result<PreparedCompletionRequest, ProviderError> {
        let name = &self.config.name;
        let mut warnings = Vec::new();
        warn_if_deprecated_key(name, &options.provider_options, &mut warnings);
        let completion: CompletionOptions =
            match merged_options(&option_keys(name), &options.provider_options) {
                Some(object) => {
                    serde_json::from_value(JsonValue::Object(object)).map_err(|error| {
                        InvalidArgumentError::new(
                            "provider_options",
                            format!("invalid {name} provider options: {error}"),
                        )
                    })?
                }
                None => CompletionOptions::default(),
            };
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
        let mut body = JsonObject::new();
        body.insert("model".to_owned(), JsonValue::from(self.model_id.as_str()));
        insert_some(&mut body, "echo", completion.echo);
        if let Some(bias) = &completion.logit_bias {
            body.insert(
                "logit_bias".to_owned(),
                serde_json::to_value(bias).map_err(ProviderError::other)?,
            );
        }
        insert_some(&mut body, "suffix", completion.suffix.clone());
        insert_some(&mut body, "user", completion.user);
        insert_some(&mut body, "max_tokens", options.max_output_tokens);
        insert_some(&mut body, "temperature", options.temperature);
        insert_some(&mut body, "top_p", options.top_p);
        insert_some(&mut body, "frequency_penalty", options.frequency_penalty);
        insert_some(&mut body, "presence_penalty", options.presence_penalty);
        insert_some(&mut body, "seed", options.seed);
        body.extend(passthrough_options(
            name,
            &options.provider_options,
            KNOWN_COMPLETION_OPTION_KEYS,
        ));
        body.insert("prompt".to_owned(), JsonValue::from(converted.prompt));
        if !stop.is_empty() {
            body.insert("stop".to_owned(), json!(stop));
        }
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
    config: SharedConfig,
    finish_reason: FinishReason,
    received_finish_reason: bool,
    usage: Option<(CompletionUsage, Option<JsonObject>)>,
    first_chunk: bool,
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
                error: stream_error_for_frame(self.config.error_structure.as_ref(), &raw),
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
            let raw_usage = raw.get("usage").and_then(JsonValue::as_object).cloned();
            self.usage = Some((usage, raw_usage));
        }
        let Some(choice) = value
            .choices
            .and_then(|mut c| (!c.is_empty()).then(|| c.remove(0)))
        else {
            return parts;
        };
        if let Some(reason) = &choice.finish_reason {
            self.received_finish_reason = true;
            self.finish_reason = map_finish_reason(reason);
        }
        if let Some(text) = choice.text.filter(|t| !t.is_empty()) {
            parts.push(StreamPart::text_delta(PartId::new(TEXT_PART_ID), text));
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
                .map_or_else(Usage::default, |(usage, raw)| {
                    convert_completion_usage(usage, raw.clone())
                }),
            provider_metadata: None,
        });
        parts
    }
}

impl LanguageModel for OpenAiCompatibleCompletionLanguageModel {
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
        let url = self.config.url("/completions");
        let handlers = ResponseHandlers::new(
            json_response_handler::<CompletionResponse>(),
            failed_response_handler(self.config.error_structure.clone()),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            self.config.headers(&options.headers)?,
            &prepared.body,
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
        let content = match choice.text.as_deref().filter(|text| !text.is_empty()) {
            Some(text) => vec![Content::text(text)],
            None => Vec::new(),
        };
        let finish_reason = choice.finish_reason.as_deref().map_or_else(
            || FinishReason::new(FinishReasonKind::Other),
            map_finish_reason,
        );
        let mut result = GenerateResult::new(content, finish_reason);
        result.usage = value.usage.as_ref().map_or_else(Usage::default, |usage| {
            convert_completion_usage(
                usage,
                raw.get("usage").and_then(JsonValue::as_object).cloned(),
            )
        });
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
        let mut prepared = self.prepare_request(&options)?;
        prepared
            .body
            .insert("stream".to_owned(), JsonValue::Bool(true));
        if self.config.include_usage {
            prepared
                .body
                .insert("stream_options".to_owned(), json!({"include_usage": true}));
        }
        let url = self.config.url("/completions");
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<CompletionResponse>(),
            failed_response_handler(self.config.error_structure.clone()),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url.clone(),
            self.config.headers(&options.headers)?,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let structure = self.config.error_structure.clone();
        let early_body = request_body.clone();
        let chunks = fail_on_early_error(response.value, classify_chunk, move |_, raw| {
            early_error(structure.as_ref(), url, raw)
                .with_request_body(early_body)
                .into()
        })
        .await?;
        let state = CompletionStreamState {
            config: self.config.clone(),
            finish_reason: FinishReason::new(FinishReasonKind::Other),
            received_finish_reason: false,
            usage: None,
            first_chunk: true,
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
