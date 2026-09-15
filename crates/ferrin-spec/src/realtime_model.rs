//! Realtime (WebSocket) model interface.
//!
//! A realtime model does not own the connection. It issues client secrets,
//! describes how to open the WebSocket, and translates between provider wire
//! events and the standardized [`RealtimeServerEvent`] /
//! [`RealtimeClientEvent`] sets. The session loop lives in the core.

use std::future::Future;

use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use crate::error::NoSuchModelError;
use crate::error::ProviderError;
use crate::json::JsonObject;
use crate::json::JsonValue;
use crate::shared::AudioFormat;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::base64_bytes;

/// A realtime model.
pub trait RealtimeModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Creates a short-lived client secret for opening a session.
    fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> impl Future<Output = Result<ClientSecret, ProviderError>> + Send;

    /// Returns the WebSocket URL and sub-protocols for a token.
    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig;

    /// Converts a raw server message into zero or more standardized events.
    ///
    /// # Errors
    ///
    /// Returns an error when the message cannot be interpreted at all;
    /// unknown but well-formed messages should map to
    /// [`RealtimeServerEvent::Custom`].
    fn parse_server_event(&self, raw: JsonValue)
    -> Result<Vec<RealtimeServerEvent>, ProviderError>;

    /// Converts a standardized client event into the provider wire format.
    fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> impl Future<Output = Result<JsonValue, ProviderError>> + Send;

    /// Converts a session configuration into the provider wire format.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration cannot be expressed.
    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError>;

    /// Returns the reply to send when `raw` is a provider health-check ping.
    fn health_check_response(&self, raw: &JsonValue) -> Option<JsonValue> {
        let _ = raw;
        None
    }
}

/// Creates realtime models and session tokens for a provider.
pub trait RealtimeFactory: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Returns the realtime model with `model_id`.
    ///
    /// # Errors
    ///
    /// Returns [`NoSuchModelError`] when the model is unknown.
    fn model(&self, model_id: &str) -> Result<crate::dynamic::RealtimeModelRef, NoSuchModelError>;

    /// Creates a session token for `options.model`.
    fn get_token(
        &self,
        options: GetTokenOptions,
    ) -> impl Future<Output = Result<ClientSecret, ProviderError>> + Send;
}

/// Options for creating a client secret.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientSecretOptions {
    /// Requested lifetime in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_seconds: Option<u64>,
    /// Initial session configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_config: Option<RealtimeSessionConfig>,
}

/// Options for [`RealtimeFactory::get_token`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetTokenOptions {
    /// Model to open the session with.
    pub model: ModelId,
    /// Requested lifetime in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_seconds: Option<u64>,
    /// Initial session configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_config: Option<RealtimeSessionConfig>,
}

/// A client secret for opening a session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSecret {
    /// The token; never logged.
    pub token: String,
    /// WebSocket URL to connect to.
    pub url: Url,
    /// Expiry as a Unix timestamp in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl std::fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSecret")
            .field("token", &"***")
            .field("url", &self.url)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// WebSocket connection parameters; URL and protocols are redacted in debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSocketConfig {
    /// URL to connect to.
    pub url: Url,
    /// Sub-protocols to request.
    pub protocols: Vec<String>,
}

impl std::fmt::Debug for WebSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketConfig")
            .field("url", &"***")
            .field("protocols", &"***")
            .finish()
    }
}

/// Output modality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Modality {
    /// Text.
    Text,
    /// Audio.
    Audio,
}

/// Transcription settings for input or output audio.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    /// Transcription model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Language hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Prompt hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Turn detection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TurnDetectionKind {
    /// Server-side voice activity detection.
    ServerVad,
    /// Semantic voice activity detection.
    SemanticVad,
    /// No automatic turn detection.
    Disabled,
}

/// Turn detection settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnDetection {
    /// Mode.
    #[serde(rename = "type")]
    pub kind: TurnDetectionKind,
    /// Activation threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// Silence duration in milliseconds that ends a turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u64>,
    /// Audio kept before detected speech, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u64>,
}

/// A function tool exposed in a realtime session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeToolDefinition {
    /// Tool name.
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema of the parameters.
    pub parameters: JsonValue,
}

/// Session configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealtimeSessionConfig {
    /// System instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Voice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Output modalities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_modalities: Option<Vec<Modality>>,
    /// Input audio format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<AudioFormat>,
    /// Input transcription settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<TranscriptionConfig>,
    /// Output transcription settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_audio_transcription: Option<TranscriptionConfig>,
    /// Output audio format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_audio_format: Option<AudioFormat>,
    /// Turn detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<TurnDetection>,
    /// Tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<RealtimeToolDefinition>,
    /// Provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<JsonObject>,
}

/// Role of a conversation item created by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ConversationRole {
    /// The user.
    User,
}

/// A conversation item created by the client, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ConversationItem {
    /// A text message.
    TextMessage {
        /// Role.
        role: ConversationRole,
        /// Text.
        text: String,
    },
    /// An audio message.
    AudioMessage {
        /// Role.
        role: ConversationRole,
        /// Audio bytes.
        #[serde(with = "base64_bytes")]
        audio: Bytes,
    },
    /// Output of a function call.
    FunctionCallOutput {
        /// Call id.
        call_id: String,
        /// Function name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Output text.
        output: String,
    },
}

/// Options of a `response-create` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseCreateOptions {
    /// Modalities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<String>>,
    /// Instructions for this response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonObject>,
}

/// Events sent by the client, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RealtimeClientEvent {
    /// Update the session configuration.
    SessionUpdate {
        /// New configuration.
        config: Box<RealtimeSessionConfig>,
    },
    /// Append input audio.
    InputAudioAppend {
        /// Audio bytes.
        #[serde(with = "base64_bytes")]
        audio: Bytes,
    },
    /// Commit the input audio buffer.
    InputAudioCommit,
    /// Clear the input audio buffer.
    InputAudioClear,
    /// Create a conversation item.
    ConversationItemCreate {
        /// The item.
        item: ConversationItem,
    },
    /// Truncate a conversation item.
    ConversationItemTruncate {
        /// Item id.
        item_id: String,
        /// Content index.
        content_index: u32,
        /// Audio end in milliseconds.
        audio_end_ms: u64,
    },
    /// Request a response.
    ResponseCreate {
        /// Options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<ResponseCreateOptions>,
    },
    /// Cancel the in-progress response.
    ResponseCancel,
}

/// Events received from the server, tagged by `type`.
///
/// Every variant carries the raw provider message in `raw`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RealtimeServerEvent {
    /// Session created.
    SessionCreated {
        /// Session id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Session updated.
    SessionUpdated {
        /// Raw message.
        raw: JsonValue,
    },
    /// Speech started.
    SpeechStarted {
        /// Item id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Speech stopped.
    SpeechStopped {
        /// Item id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Input audio committed.
    AudioCommitted {
        /// Item id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        /// Previous item id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_item_id: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Conversation item added.
    ConversationItemAdded {
        /// Item id.
        item_id: String,
        /// The item.
        item: JsonValue,
        /// Raw message.
        raw: JsonValue,
    },
    /// Input transcription completed.
    InputTranscriptionCompleted {
        /// Item id.
        item_id: String,
        /// Transcript.
        transcript: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Response created.
    ResponseCreated {
        /// Response id.
        response_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Response done.
    ResponseDone {
        /// Response id.
        response_id: String,
        /// Status.
        status: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Output item added.
    OutputItemAdded {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Output item done.
    OutputItemDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Content part added.
    ContentPartAdded {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Content part done.
    ContentPartDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Audio increment.
    AudioDelta {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Audio bytes.
        #[serde(with = "base64_bytes")]
        delta: Bytes,
        /// Raw message.
        raw: JsonValue,
    },
    /// Audio done.
    AudioDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Audio transcript increment.
    AudioTranscriptDelta {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Text.
        delta: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Audio transcript done.
    AudioTranscriptDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Full transcript.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Text increment.
    TextDelta {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Text.
        delta: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Text done.
    TextDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Full text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// Function call arguments increment.
    FunctionCallArgumentsDelta {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Call id.
        call_id: String,
        /// Arguments text.
        delta: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Function call arguments done.
    FunctionCallArgumentsDone {
        /// Response id.
        response_id: String,
        /// Item id.
        item_id: String,
        /// Call id.
        call_id: String,
        /// Function name.
        name: String,
        /// Complete arguments JSON text.
        arguments: String,
        /// Raw message.
        raw: JsonValue,
    },
    /// Error.
    Error {
        /// Message.
        message: String,
        /// Code.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// Raw message.
        raw: JsonValue,
    },
    /// A provider event without a standardized mapping.
    Custom {
        /// Provider event type.
        raw_type: String,
        /// Raw message.
        raw: JsonValue,
    },
}
