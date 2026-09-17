//! Live API realtime model: ephemeral auth tokens, session setup and the
//! mapping between Live API messages and the specification events.

use std::sync::Mutex;

use base64::Engine;
use bytes::Bytes;
use chrono::Duration;
use chrono::Utc;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RealtimeModelRef;
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
use ferrin_spec::realtime_model::WebSocketConfig;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::config::AUTH_TOKENS_PATH;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::GoogleConfig;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::json_schema::convert_json_schema_to_openapi_schema;

/// Provider id family.
pub const FAMILY: &str = "realtime";

/// Service path of the constrained (token-authenticated) bidi endpoint.
pub const WEBSOCKET_SERVICE_PATH: &str =
    "google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained";

/// Default window in which a token may open a session.
pub const DEFAULT_EXPIRES_AFTER_SECONDS: u64 = 60;

/// Default input audio sample rate advertised in audio blobs.
pub const DEFAULT_INPUT_AUDIO_RATE: u32 = 16_000;

#[derive(Debug, Deserialize)]
struct AuthTokenResponse {
    name: String,
    #[serde(default, rename = "expireTime")]
    expire_time: Option<String>,
}

/// Builds the `setup` message (`bidiGenerateContentSetup`) for a session.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] when a tool schema
/// cannot be converted.
pub fn build_session_config(
    config: Option<&RealtimeSessionConfig>,
    model_id: &str,
) -> Result<JsonValue, ProviderError> {
    let mut setup = JsonObject::new();
    setup.insert(
        "model".to_owned(),
        JsonValue::from(GoogleConfig::model_path(model_id)),
    );
    let mut generation = JsonObject::new();
    let modalities: Vec<JsonValue> =
        match config.and_then(|config| config.output_modalities.as_ref()) {
            Some(modalities) => modalities
                .iter()
                .map(|modality| {
                    let text = serde_json::to_value(modality)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_ascii_uppercase))
                        .unwrap_or_else(|| "AUDIO".to_owned());
                    JsonValue::from(text)
                })
                .collect(),
            None => vec![JsonValue::from("AUDIO")],
        };
    generation.insert(
        "responseModalities".to_owned(),
        JsonValue::Array(modalities),
    );
    if let Some(voice) = config.and_then(|config| config.voice.as_deref()) {
        generation.insert(
            "speechConfig".to_owned(),
            json!({"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": voice}}}),
        );
    }
    setup.insert("generationConfig".to_owned(), JsonValue::Object(generation));
    let Some(config) = config else {
        return Ok(JsonValue::Object(setup));
    };
    if let Some(instructions) = &config.instructions {
        setup.insert(
            "systemInstruction".to_owned(),
            json!({"parts": [{"text": instructions}]}),
        );
    }
    if !config.tools.is_empty() {
        let mut declarations = Vec::new();
        for tool in &config.tools {
            let mut declaration = JsonObject::new();
            declaration.insert("name".to_owned(), JsonValue::from(tool.name.as_str()));
            if let Some(description) = &tool.description {
                declaration.insert(
                    "description".to_owned(),
                    JsonValue::from(description.as_str()),
                );
            }
            if let Some(parameters) = convert_json_schema_to_openapi_schema(&tool.parameters)? {
                declaration.insert("parameters".to_owned(), parameters);
            }
            declarations.push(JsonValue::Object(declaration));
        }
        setup.insert(
            "tools".to_owned(),
            json!([{"functionDeclarations": declarations}]),
        );
    }
    if config.input_audio_transcription.is_some() {
        setup.insert("inputAudioTranscription".to_owned(), json!({}));
    }
    if config.output_audio_transcription.is_some() {
        setup.insert("outputAudioTranscription".to_owned(), json!({}));
    }
    if let Some(provider_options) = &config.provider_options {
        let mut google: Option<JsonObject> = None;
        for (key, value) in provider_options {
            if key == CANONICAL_OPTIONS_KEY {
                if let JsonValue::Object(object) = value {
                    google = Some(object.clone());
                }
            } else {
                setup.insert(key.clone(), value.clone());
            }
        }
        if let Some(translation) = google
            .as_ref()
            .and_then(|google| google.get("translationConfig"))
        {
            let target = match setup.get_mut("generationConfig") {
                Some(JsonValue::Object(generation)) => generation,
                _ => {
                    setup.insert("generationConfig".to_owned(), json!({}));
                    match setup.get_mut("generationConfig") {
                        Some(JsonValue::Object(generation)) => generation,
                        _ => return Ok(JsonValue::Object(setup)),
                    }
                }
            };
            target.insert("translationConfig".to_owned(), translation.clone());
        }
    }
    Ok(JsonValue::Object(setup))
}

/// Turn tracking of the stateful event mapper.
#[derive(Debug, Default)]
struct MapperState {
    turn_counter: u64,
    has_audio: bool,
    has_text: bool,
    has_transcript: bool,
    turn_closed: bool,
    input_audio_rate: Option<u32>,
}

impl MapperState {
    fn response_id(&self) -> String {
        format!("google-resp-{}", self.turn_counter)
    }

    fn item_id(&self) -> String {
        format!("google-item-{}", self.turn_counter)
    }

    fn input_id(&self) -> String {
        format!("google-input-{}", self.turn_counter)
    }

    fn begin_turn_if_closed(&mut self) {
        if self.turn_closed {
            self.turn_counter += 1;
            self.has_audio = false;
            self.has_text = false;
            self.has_transcript = false;
            self.turn_closed = false;
        }
    }
}

fn custom(raw_type: &str, raw: &JsonValue) -> RealtimeServerEvent {
    RealtimeServerEvent::Custom {
        raw_type: raw_type.to_owned(),
        raw: raw.clone(),
    }
}

fn decode_audio(data: &str) -> Bytes {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map(Bytes::from)
        .unwrap_or_default()
}

fn parse_server_content(
    state: &mut MapperState,
    content: &JsonValue,
    raw: &JsonValue,
) -> Vec<RealtimeServerEvent> {
    let mut events = Vec::new();
    if content.get("interrupted").and_then(JsonValue::as_bool) == Some(true) {
        events.push(RealtimeServerEvent::SpeechStarted {
            item_id: None,
            raw: raw.clone(),
        });
    }
    if let Some(parts) = content
        .get("modelTurn")
        .and_then(|turn| turn.get("parts"))
        .and_then(JsonValue::as_array)
    {
        state.begin_turn_if_closed();
        for part in parts {
            if let Some(data) = part
                .get("inlineData")
                .and_then(|inline| inline.get("data"))
                .and_then(JsonValue::as_str)
                .filter(|data| !data.is_empty())
            {
                state.has_audio = true;
                events.push(RealtimeServerEvent::AudioDelta {
                    response_id: state.response_id(),
                    item_id: state.item_id(),
                    delta: decode_audio(data),
                    raw: raw.clone(),
                });
            }
            if let Some(text) = part
                .get("text")
                .and_then(JsonValue::as_str)
                .filter(|text| !text.is_empty())
            {
                state.has_text = true;
                events.push(RealtimeServerEvent::TextDelta {
                    response_id: state.response_id(),
                    item_id: state.item_id(),
                    delta: text.to_owned(),
                    raw: raw.clone(),
                });
            }
        }
    }
    if let Some(text) = content
        .get("outputTranscription")
        .and_then(|transcription| transcription.get("text"))
        .and_then(JsonValue::as_str)
        .filter(|text| !text.is_empty())
    {
        state.has_transcript = true;
        events.push(RealtimeServerEvent::AudioTranscriptDelta {
            response_id: state.response_id(),
            item_id: state.item_id(),
            delta: text.to_owned(),
            raw: raw.clone(),
        });
    }
    if let Some(text) = content
        .get("inputTranscription")
        .and_then(|transcription| transcription.get("text"))
        .and_then(JsonValue::as_str)
        .filter(|text| !text.is_empty())
    {
        events.push(RealtimeServerEvent::InputTranscriptionCompleted {
            item_id: state.input_id(),
            transcript: text.to_owned(),
            raw: raw.clone(),
        });
    }
    if content
        .get("generationComplete")
        .and_then(JsonValue::as_bool)
        == Some(true)
    {
        events.push(custom("generationComplete", raw));
    }
    if content.get("turnComplete").and_then(JsonValue::as_bool) == Some(true) {
        if state.has_audio {
            events.push(RealtimeServerEvent::AudioDone {
                response_id: state.response_id(),
                item_id: state.item_id(),
                raw: raw.clone(),
            });
        }
        if state.has_text {
            events.push(RealtimeServerEvent::TextDone {
                response_id: state.response_id(),
                item_id: state.item_id(),
                text: None,
                raw: raw.clone(),
            });
        }
        if state.has_transcript {
            events.push(RealtimeServerEvent::AudioTranscriptDone {
                response_id: state.response_id(),
                item_id: state.item_id(),
                transcript: None,
                raw: raw.clone(),
            });
        }
        events.push(RealtimeServerEvent::ResponseDone {
            response_id: state.response_id(),
            status: "completed".to_owned(),
            raw: raw.clone(),
        });
        state.turn_closed = true;
    }
    if events.is_empty() {
        events.push(custom("serverContent", raw));
    }
    events
}

fn parse_event(state: &mut MapperState, raw: &JsonValue) -> Vec<RealtimeServerEvent> {
    let Some(object) = raw.as_object() else {
        return vec![custom("unknown", raw)];
    };
    if object.contains_key("setupComplete") {
        return vec![RealtimeServerEvent::SessionCreated {
            session_id: None,
            raw: raw.clone(),
        }];
    }
    if let Some(tool_call) = object.get("toolCall") {
        state.begin_turn_if_closed();
        let mut events = Vec::new();
        for call in tool_call
            .get("functionCalls")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
        {
            let args = call
                .get("args")
                .cloned()
                .unwrap_or_else(|| JsonValue::Object(JsonObject::new()))
                .to_string();
            let call_id = call
                .get("id")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let name = call
                .get("name")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_owned();
            events.push(RealtimeServerEvent::FunctionCallArgumentsDelta {
                response_id: state.response_id(),
                item_id: state.item_id(),
                call_id: call_id.clone(),
                delta: args.clone(),
                raw: raw.clone(),
            });
            events.push(RealtimeServerEvent::FunctionCallArgumentsDone {
                response_id: state.response_id(),
                item_id: state.item_id(),
                call_id,
                name,
                arguments: args,
                raw: raw.clone(),
            });
        }
        return events;
    }
    for key in ["toolCallCancellation", "goAway", "sessionResumptionUpdate"] {
        if object.contains_key(key) {
            return vec![custom(key, raw)];
        }
    }
    if let Some(content) = object.get("serverContent") {
        return parse_server_content(state, content, raw);
    }
    if let Some(text) = object
        .get("inputTranscription")
        .and_then(|transcription| transcription.get("text"))
        .and_then(JsonValue::as_str)
    {
        return vec![RealtimeServerEvent::InputTranscriptionCompleted {
            item_id: state.input_id(),
            transcript: text.to_owned(),
            raw: raw.clone(),
        }];
    }
    let raw_type = object.keys().next().map_or("unknown", String::as_str);
    vec![custom(raw_type, raw)]
}

fn serialize_event(
    state: &mut MapperState,
    event: RealtimeClientEvent,
    model_id: &str,
) -> Result<JsonValue, ProviderError> {
    Ok(match event {
        RealtimeClientEvent::SessionUpdate { config } => {
            if let Some(rate) = config
                .input_audio_format
                .as_ref()
                .and_then(|format| format.rate)
            {
                state.input_audio_rate = Some(rate);
            }
            json!({"setup": build_session_config(Some(&config), model_id)?})
        }
        RealtimeClientEvent::InputAudioAppend { audio } => {
            let rate = state.input_audio_rate.unwrap_or(DEFAULT_INPUT_AUDIO_RATE);
            json!({"realtimeInput": {"audio": {
                "data": base64::engine::general_purpose::STANDARD.encode(&audio),
                "mimeType": format!("audio/pcm;rate={rate}"),
            }}})
        }
        RealtimeClientEvent::InputAudioCommit => {
            json!({"realtimeInput": {"audioStreamEnd": true}})
        }
        RealtimeClientEvent::ConversationItemCreate { item } => match item {
            ConversationItem::TextMessage { text, .. } => {
                json!({"realtimeInput": {"text": text}})
            }
            ConversationItem::FunctionCallOutput {
                call_id,
                name,
                output,
            } => {
                let response = ferrin_schema::json::parse(&output).unwrap_or_else(|_| json!({}));
                let mut function_response = JsonObject::new();
                function_response.insert("id".to_owned(), JsonValue::from(call_id));
                if let Some(name) = name {
                    function_response.insert("name".to_owned(), JsonValue::from(name));
                }
                function_response.insert("response".to_owned(), response);
                json!({"toolResponse": {"functionResponses": [function_response]}})
            }
            ConversationItem::AudioMessage { .. } => JsonValue::Null,
            #[allow(unreachable_patterns, reason = "ConversationItem is non-exhaustive")]
            _ => return Err(ProviderError::unsupported("realtime conversation item")),
        },
        RealtimeClientEvent::InputAudioClear
        | RealtimeClientEvent::ResponseCreate { .. }
        | RealtimeClientEvent::ResponseCancel
        | RealtimeClientEvent::ConversationItemTruncate { .. } => JsonValue::Null,
        #[allow(unreachable_patterns, reason = "RealtimeClientEvent is non-exhaustive")]
        _ => return Err(ProviderError::unsupported("realtime client event")),
    })
}

/// Live API realtime model.
#[derive(Debug)]
pub struct GoogleRealtimeModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
    state: Mutex<MapperState>,
}

impl GoogleRealtimeModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id(FAMILY),
            config,
            model_id: model_id.into(),
            state: Mutex::new(MapperState::default()),
        }
    }

    /// WebSocket URL of the constrained bidi endpoint (without the token).
    #[must_use]
    pub fn session_url(&self) -> Url {
        self.config.websocket_url(WEBSOCKET_SERVICE_PATH)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MapperState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RealtimeModel for GoogleRealtimeModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> Result<ClientSecret, ProviderError> {
        let api_key = self.config.api_key()?;
        let now = Utc::now();
        let window = i64::try_from(
            options
                .expires_after_seconds
                .unwrap_or(DEFAULT_EXPIRES_AFTER_SECONDS),
        )
        .unwrap_or(i64::MAX / 4);
        let new_session_expire_time = now + Duration::seconds(window);
        let expire_time = new_session_expire_time + Duration::minutes(30);
        let body = json!({
            "uses": 0,
            "expireTime": expire_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "newSessionExpireTime": new_session_expire_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "bidiGenerateContentSetup": build_session_config(
                options.session_config.as_ref(),
                self.model_id.as_str(),
            )?,
        });
        let mut url = self.config.origin_url(AUTH_TOKENS_PATH);
        url.query_pairs_mut()
            .append_pair("key", api_key.expose_secret());
        let mut headers = self.config.headers.clone();
        headers = headers.with_user_agent_suffix([crate::config::USER_AGENT]);
        let handlers = ResponseHandlers::new(
            json_response_handler::<AuthTokenResponse>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            headers,
            &body,
            &handlers,
            CancellationToken::new(),
        )
        .await?;
        let expires_at = response
            .value
            .expire_time
            .as_deref()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .and_then(|time| u64::try_from(time.timestamp()).ok());
        Ok(ClientSecret {
            token: response.value.name,
            url: self.session_url(),
            expires_at,
        })
    }

    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig {
        let mut url = url.clone();
        url.query_pairs_mut().append_pair("access_token", token);
        WebSocketConfig {
            url,
            protocols: Vec::new(),
        }
    }

    fn parse_server_event(
        &self,
        raw: JsonValue,
    ) -> Result<Vec<RealtimeServerEvent>, ProviderError> {
        let mut state = self.lock();
        Ok(parse_event(&mut state, &raw))
    }

    async fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> Result<JsonValue, ProviderError> {
        let mut state = self.lock();
        serialize_event(&mut state, event, self.model_id.as_str())
    }

    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError> {
        build_session_config(Some(config), self.model_id.as_str())
    }
}

/// Factory for Live API models and session tokens.
#[derive(Debug, Clone)]
pub struct GoogleRealtimeFactory {
    config: SharedConfig,
    provider: ProviderId,
}

impl GoogleRealtimeFactory {
    /// Creates the factory.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id(FAMILY),
            config,
        }
    }

    /// Creates a realtime model.
    #[must_use]
    pub fn realtime_model(&self, model_id: &str) -> GoogleRealtimeModel {
        GoogleRealtimeModel::new(self.config.clone(), model_id)
    }
}

impl RealtimeFactory for GoogleRealtimeFactory {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model(&self, model_id: &str) -> Result<RealtimeModelRef, NoSuchModelError> {
        Ok(self.realtime_model(model_id).into())
    }

    async fn get_token(&self, options: GetTokenOptions) -> Result<ClientSecret, ProviderError> {
        self.realtime_model(options.model.as_str())
            .do_create_client_secret(ClientSecretOptions {
                expires_after_seconds: options.expires_after_seconds,
                session_config: options.session_config,
            })
            .await
    }
}

/// Headers of the auth token request: configuration headers only (the key
/// travels in the query string).
#[must_use]
pub fn token_request_headers(config: &GoogleConfig) -> Headers {
    config
        .headers
        .clone()
        .with_user_agent_suffix([crate::config::USER_AGENT])
}
