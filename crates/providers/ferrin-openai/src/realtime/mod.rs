//! Realtime model (`<name>.realtime`): client secrets, session
//! configuration and event mapping. The WebSocket session loop lives in
//! `ferrin-core`.

use base64::Engine;
use bytes::Bytes;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RealtimeModelRef;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::realtime_model::ClientSecret;
use ferrin_spec::realtime_model::ClientSecretOptions;
use ferrin_spec::realtime_model::ConversationItem;
use ferrin_spec::realtime_model::GetTokenOptions;
use ferrin_spec::realtime_model::RealtimeClientEvent;
use ferrin_spec::realtime_model::RealtimeFactory;
use ferrin_spec::realtime_model::RealtimeModel;
use ferrin_spec::realtime_model::RealtimeServerEvent;
use ferrin_spec::realtime_model::RealtimeSessionConfig;
use ferrin_spec::realtime_model::TurnDetectionKind;
use ferrin_spec::realtime_model::WebSocketConfig;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;

/// Default input transcription model.
const DEFAULT_TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";

/// Response of `POST /realtime/client_secrets`.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientSecretResponse {
    /// Ephemeral token.
    pub value: String,
    /// Expiry (Unix seconds).
    #[serde(default)]
    pub expires_at: Option<u64>,
}

/// Sub-protocols announcing the token.
#[must_use]
pub fn protocols(token: &str) -> Vec<String> {
    vec![
        "realtime".to_owned(),
        format!("openai-insecure-api-key.{token}"),
    ]
}

fn audio_format(format: &ferrin_spec::AudioFormat) -> JsonValue {
    let mut value = json!({"type": format.kind});
    if let Some(rate) = format.rate
        && let Some(object) = value.as_object_mut()
    {
        object.insert("rate".to_owned(), JsonValue::from(rate));
    }
    value
}

/// Builds the wire session object.
#[must_use]
pub fn session_config(config: &RealtimeSessionConfig, model_id: &str) -> JsonValue {
    let mut session = JsonObject::new();
    session.insert("type".to_owned(), JsonValue::from("realtime"));
    session.insert("model".to_owned(), JsonValue::from(model_id));
    if let Some(instructions) = &config.instructions {
        session.insert(
            "instructions".to_owned(),
            JsonValue::from(instructions.as_str()),
        );
    }
    if let Some(modalities) = &config.output_modalities {
        session.insert(
            "output_modalities".to_owned(),
            serde_json::to_value(modalities).unwrap_or(JsonValue::Null),
        );
    }
    let mut audio = JsonObject::new();
    if config.input_audio_format.is_some()
        || config.input_audio_transcription.is_some()
        || config.turn_detection.is_some()
    {
        let mut input = JsonObject::new();
        if let Some(format) = &config.input_audio_format {
            input.insert("format".to_owned(), audio_format(format));
        }
        if let Some(turn_detection) = &config.turn_detection {
            let value = match turn_detection.kind {
                TurnDetectionKind::Disabled => JsonValue::Null,
                kind => {
                    let mut td = JsonObject::new();
                    td.insert(
                        "type".to_owned(),
                        JsonValue::from(match kind {
                            TurnDetectionKind::SemanticVad => "semantic_vad",
                            _ => "server_vad",
                        }),
                    );
                    if let Some(threshold) = turn_detection.threshold {
                        td.insert("threshold".to_owned(), json!(threshold));
                    }
                    if let Some(ms) = turn_detection.silence_duration_ms {
                        td.insert("silence_duration_ms".to_owned(), json!(ms));
                    }
                    if let Some(ms) = turn_detection.prefix_padding_ms {
                        td.insert("prefix_padding_ms".to_owned(), json!(ms));
                    }
                    JsonValue::Object(td)
                }
            };
            input.insert("turn_detection".to_owned(), value);
        }
        if let Some(transcription) = &config.input_audio_transcription {
            let mut value = JsonObject::new();
            value.insert(
                "model".to_owned(),
                JsonValue::from(
                    transcription
                        .model
                        .as_deref()
                        .unwrap_or(DEFAULT_TRANSCRIPTION_MODEL),
                ),
            );
            if let Some(language) = &transcription.language {
                value.insert("language".to_owned(), JsonValue::from(language.as_str()));
            }
            if let Some(prompt) = &transcription.prompt {
                value.insert("prompt".to_owned(), JsonValue::from(prompt.as_str()));
            }
            input.insert("transcription".to_owned(), JsonValue::Object(value));
        }
        audio.insert("input".to_owned(), JsonValue::Object(input));
    }
    if config.output_audio_format.is_some() || config.voice.is_some() {
        let mut output = JsonObject::new();
        if let Some(format) = &config.output_audio_format {
            output.insert("format".to_owned(), audio_format(format));
        }
        if let Some(voice) = &config.voice {
            output.insert("voice".to_owned(), JsonValue::from(voice.as_str()));
        }
        audio.insert("output".to_owned(), JsonValue::Object(output));
    }
    if !audio.is_empty() {
        session.insert("audio".to_owned(), JsonValue::Object(audio));
    }
    if !config.tools.is_empty() {
        let tools: Vec<JsonValue> = config
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            })
            .collect();
        session.insert("tools".to_owned(), JsonValue::Array(tools));
        session.insert("tool_choice".to_owned(), JsonValue::from("auto"));
    }
    if let Some(options) = &config.provider_options {
        for (key, value) in options {
            session.insert(key.clone(), value.clone());
        }
    }
    JsonValue::Object(session)
}

/// Serializes a client event.
#[must_use]
pub fn client_event(event: RealtimeClientEvent, model_id: &str) -> JsonValue {
    match event {
        RealtimeClientEvent::SessionUpdate { config } => json!({
            "type": "session.update",
            "session": session_config(&config, model_id),
        }),
        RealtimeClientEvent::InputAudioAppend { audio } => json!({
            "type": "input_audio_buffer.append",
            "audio": base64::engine::general_purpose::STANDARD.encode(&audio),
        }),
        RealtimeClientEvent::InputAudioCommit => json!({"type": "input_audio_buffer.commit"}),
        RealtimeClientEvent::InputAudioClear => json!({"type": "input_audio_buffer.clear"}),
        RealtimeClientEvent::ConversationItemCreate { item } => {
            let item = match item {
                ConversationItem::TextMessage { role, text } => json!({
                    "type": "message",
                    "role": role,
                    "content": [{"type": "input_text", "text": text}],
                }),
                ConversationItem::AudioMessage { role, audio } => json!({
                    "type": "message",
                    "role": role,
                    "content": [{
                        "type": "input_audio",
                        "audio": base64::engine::general_purpose::STANDARD.encode(&audio),
                    }],
                }),
                ConversationItem::FunctionCallOutput {
                    call_id, output, ..
                } => json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }),
                #[allow(unreachable_patterns, reason = "ConversationItem is non-exhaustive")]
                _ => JsonValue::Null,
            };
            json!({"type": "conversation.item.create", "item": item})
        }
        RealtimeClientEvent::ConversationItemTruncate {
            item_id,
            content_index,
            audio_end_ms,
        } => json!({
            "type": "conversation.item.truncate",
            "item_id": item_id,
            "content_index": content_index,
            "audio_end_ms": audio_end_ms,
        }),
        RealtimeClientEvent::ResponseCreate { options } => {
            let mut event = json!({"type": "response.create"});
            if let Some(options) = options {
                let mut response = JsonObject::new();
                if let Some(modalities) = options.modalities {
                    response.insert("output_modalities".to_owned(), json!(modalities));
                }
                if let Some(instructions) = options.instructions {
                    response.insert("instructions".to_owned(), JsonValue::from(instructions));
                }
                if let Some(metadata) = options.metadata {
                    response.insert("metadata".to_owned(), JsonValue::Object(metadata));
                }
                if let Some(object) = event.as_object_mut() {
                    object.insert("response".to_owned(), JsonValue::Object(response));
                }
            }
            event
        }
        RealtimeClientEvent::ResponseCancel => json!({"type": "response.cancel"}),
        #[allow(unreachable_patterns, reason = "RealtimeClientEvent is non-exhaustive")]
        _ => JsonValue::Null,
    }
}

fn str_field(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

fn nested_str(value: &JsonValue, object: &str, key: &str) -> Option<String> {
    value
        .get(object)
        .and_then(|inner| inner.get(key))
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

/// Maps a raw server event.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidResponseData`] when an audio delta is not
/// valid base64.
pub fn server_event(raw: JsonValue) -> Result<RealtimeServerEvent, ProviderError> {
    let kind = str_field(&raw, "type").unwrap_or_default();
    let item_id = || str_field(&raw, "item_id").unwrap_or_default();
    let response_id = || str_field(&raw, "response_id").unwrap_or_default();
    let item_or_event_id = || nested_str(&raw, "item", "id").unwrap_or_else(item_id);
    let response_or_event_id = || nested_str(&raw, "response", "id").unwrap_or_else(response_id);
    let delta = || str_field(&raw, "delta").unwrap_or_default();
    let event = match kind.as_str() {
        "session.created" => RealtimeServerEvent::SessionCreated {
            session_id: nested_str(&raw, "session", "id"),
            raw,
        },
        "session.updated" => RealtimeServerEvent::SessionUpdated { raw },
        "input_audio_buffer.speech_started" => RealtimeServerEvent::SpeechStarted {
            item_id: str_field(&raw, "item_id"),
            raw,
        },
        "input_audio_buffer.speech_stopped" => RealtimeServerEvent::SpeechStopped {
            item_id: str_field(&raw, "item_id"),
            raw,
        },
        "input_audio_buffer.committed" => RealtimeServerEvent::AudioCommitted {
            item_id: str_field(&raw, "item_id"),
            previous_item_id: str_field(&raw, "previous_item_id"),
            raw,
        },
        "conversation.item.added" => RealtimeServerEvent::ConversationItemAdded {
            item_id: item_or_event_id(),
            item: raw.get("item").cloned().unwrap_or(JsonValue::Null),
            raw,
        },
        "conversation.item.input_audio_transcription.completed" => {
            RealtimeServerEvent::InputTranscriptionCompleted {
                item_id: item_id(),
                transcript: str_field(&raw, "transcript").unwrap_or_default(),
                raw,
            }
        }
        "response.created" => RealtimeServerEvent::ResponseCreated {
            response_id: response_or_event_id(),
            raw,
        },
        "response.done" => RealtimeServerEvent::ResponseDone {
            response_id: response_or_event_id(),
            status: nested_str(&raw, "response", "status")
                .unwrap_or_else(|| "completed".to_owned()),
            raw,
        },
        "response.output_item.added" => RealtimeServerEvent::OutputItemAdded {
            response_id: response_id(),
            item_id: item_or_event_id(),
            raw,
        },
        "response.output_item.done" => RealtimeServerEvent::OutputItemDone {
            response_id: response_id(),
            item_id: item_or_event_id(),
            raw,
        },
        "response.content_part.added" => RealtimeServerEvent::ContentPartAdded {
            response_id: response_id(),
            item_id: item_id(),
            raw,
        },
        "response.content_part.done" => RealtimeServerEvent::ContentPartDone {
            response_id: response_id(),
            item_id: item_id(),
            raw,
        },
        "response.output_audio.delta" => {
            let audio = base64::engine::general_purpose::STANDARD
                .decode(delta())
                .map_err(|error| {
                    InvalidResponseDataError::new(
                        format!("audio delta is not valid base64: {error}"),
                        raw.clone(),
                    )
                })?;
            RealtimeServerEvent::AudioDelta {
                response_id: response_id(),
                item_id: item_id(),
                delta: Bytes::from(audio),
                raw,
            }
        }
        "response.output_audio.done" => RealtimeServerEvent::AudioDone {
            response_id: response_id(),
            item_id: item_id(),
            raw,
        },
        "response.output_audio_transcript.delta" => RealtimeServerEvent::AudioTranscriptDelta {
            response_id: response_id(),
            item_id: item_id(),
            delta: delta(),
            raw,
        },
        "response.output_audio_transcript.done" => RealtimeServerEvent::AudioTranscriptDone {
            response_id: response_id(),
            item_id: item_id(),
            transcript: str_field(&raw, "transcript"),
            raw,
        },
        "response.output_text.delta" => RealtimeServerEvent::TextDelta {
            response_id: response_id(),
            item_id: item_id(),
            delta: delta(),
            raw,
        },
        "response.output_text.done" => RealtimeServerEvent::TextDone {
            response_id: response_id(),
            item_id: item_id(),
            text: str_field(&raw, "text"),
            raw,
        },
        "response.function_call_arguments.delta" => {
            RealtimeServerEvent::FunctionCallArgumentsDelta {
                response_id: response_id(),
                item_id: item_id(),
                call_id: str_field(&raw, "call_id").unwrap_or_default(),
                delta: delta(),
                raw,
            }
        }
        "response.function_call_arguments.done" => RealtimeServerEvent::FunctionCallArgumentsDone {
            response_id: response_id(),
            item_id: item_id(),
            call_id: str_field(&raw, "call_id").unwrap_or_default(),
            name: str_field(&raw, "name").unwrap_or_default(),
            arguments: str_field(&raw, "arguments").unwrap_or_default(),
            raw,
        },
        "error" => RealtimeServerEvent::Error {
            message: nested_str(&raw, "error", "message")
                .or_else(|| str_field(&raw, "message"))
                .unwrap_or_else(|| "Unknown error".to_owned()),
            code: nested_str(&raw, "error", "code").or_else(|| str_field(&raw, "code")),
            raw,
        },
        _ => RealtimeServerEvent::Custom {
            raw_type: kind,
            raw,
        },
    };
    Ok(event)
}

/// Realtime model.
#[derive(Debug, Clone)]
pub struct OpenAiRealtimeModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiRealtimeModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("realtime"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// WebSocket URL of a session (`wss://<base>/realtime?model=<id>`).
    #[must_use]
    pub fn session_url(&self) -> Url {
        self.config
            .websocket_url("/realtime", &[("model", self.model_id.as_str())])
    }
}

impl RealtimeModel for OpenAiRealtimeModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> Result<ClientSecret, ProviderError> {
        let session = options.session_config.as_ref().map_or_else(
            || json!({"type": "realtime", "model": self.model_id.as_str()}),
            |config| session_config(config, self.model_id.as_str()),
        );
        let mut body = json!({"session": session});
        if let Some(seconds) = options.expires_after_seconds
            && let Some(object) = body.as_object_mut()
        {
            object.insert(
                "expires_after".to_owned(),
                json!({"anchor": "created_at", "seconds": seconds}),
            );
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<ClientSecretResponse>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("/realtime/client_secrets"),
            self.config.headers(&Headers::new())?,
            &body,
            &handlers,
            tokio_util::sync::CancellationToken::new(),
        )
        .await?;
        Ok(ClientSecret {
            token: response.value.value,
            url: self.session_url(),
            expires_at: response.value.expires_at,
        })
    }

    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig {
        WebSocketConfig {
            url: url.clone(),
            protocols: protocols(token),
        }
    }

    fn parse_server_event(
        &self,
        raw: JsonValue,
    ) -> Result<Vec<RealtimeServerEvent>, ProviderError> {
        server_event(raw).map(|event| vec![event])
    }

    async fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> Result<JsonValue, ProviderError> {
        Ok(client_event(event, self.model_id.as_str()))
    }

    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError> {
        Ok(session_config(config, self.model_id.as_str()))
    }
}

/// Creates realtime models and session tokens.
#[derive(Debug, Clone)]
pub struct OpenAiRealtimeFactory {
    config: SharedConfig,
    provider: ProviderId,
}

impl OpenAiRealtimeFactory {
    /// Creates the factory.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("realtime"),
            config,
        }
    }

    /// Returns the realtime model with `model_id`.
    #[must_use]
    pub fn realtime_model(&self, model_id: impl Into<ModelId>) -> OpenAiRealtimeModel {
        OpenAiRealtimeModel::new(self.config.clone(), model_id)
    }
}

impl RealtimeFactory for OpenAiRealtimeFactory {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model(&self, model_id: &str) -> Result<RealtimeModelRef, NoSuchModelError> {
        Ok(self.realtime_model(model_id).into())
    }

    async fn get_token(&self, options: GetTokenOptions) -> Result<ClientSecret, ProviderError> {
        self.realtime_model(options.model)
            .do_create_client_secret(ClientSecretOptions {
                expires_after_seconds: options.expires_after_seconds,
                session_config: options.session_config,
            })
            .await
    }
}
