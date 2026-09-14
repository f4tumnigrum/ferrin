//! Transcription (speech-to-text) model interface.

use std::future::Future;

use bytes::Bytes;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::dynamic::BoxStream;
use crate::error::ProviderError;
use crate::json::JsonValue;
use crate::language_model::RequestMetadata;
use crate::language_model::ResponseMetadata;
use crate::language_model::StreamError;
use crate::shared::AudioFormat;
use crate::shared::Headers;
use crate::shared::MediaType;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;

/// A model that transcribes audio to text.
///
/// Streaming transcription is optional: implementations that support it
/// override [`supports_stream`](Self::supports_stream) and
/// [`do_stream`](Self::do_stream).
pub trait TranscriptionModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Transcribes a complete audio file.
    fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> impl Future<Output = Result<TranscriptionResult, ProviderError>> + Send;

    /// Whether [`do_stream`](Self::do_stream) is implemented.
    fn supports_stream(&self) -> bool {
        false
    }

    /// Transcribes a live audio stream.
    fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> impl Future<Output = Result<TranscriptionStreamResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("transcription streaming")))
    }
}

/// Options for a transcription call.
#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    /// Audio bytes.
    pub audio: Bytes,
    /// Media type of the audio.
    pub media_type: MediaType,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl TranscriptionOptions {
    /// Creates options for `audio` of `media_type`.
    #[must_use]
    pub fn new(audio: Bytes, media_type: impl Into<MediaType>) -> Self {
        Self {
            audio,
            media_type: media_type.into(),
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// A timed segment of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Segment text.
    pub text: String,
    /// Start time in seconds.
    pub start_second: f64,
    /// End time in seconds.
    pub end_second: f64,
}

/// Result of a transcription call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcript.
    pub text: String,
    /// Timed segments, if the provider reports them.
    #[serde(default)]
    pub segments: Vec<TranscriptionSegment>,
    /// Detected language, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Audio duration in seconds, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_in_seconds: Option<f64>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Request metadata.
    #[serde(default)]
    pub request: RequestMetadata,
    /// Response metadata; `timestamp` and `model_id` are expected to be set.
    #[serde(default)]
    pub response: ResponseMetadata,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Options for a streaming transcription call.
pub struct TranscriptionStreamOptions {
    /// Audio chunks.
    pub audio: BoxStream<'static, Bytes>,
    /// Format of the audio chunks.
    pub input_audio_format: AudioFormat,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Whether to include `Raw` parts.
    pub include_raw_chunks: bool,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl std::fmt::Debug for TranscriptionStreamOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionStreamOptions")
            .field("audio", &"<stream>")
            .field("input_audio_format", &self.input_audio_format)
            .field("provider_options", &self.provider_options)
            .field("headers", &self.headers)
            .field("include_raw_chunks", &self.include_raw_chunks)
            .finish_non_exhaustive()
    }
}

/// A part of a transcription stream, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TranscriptionStreamPart {
    /// First part of every stream.
    StreamStart {
        /// Warnings.
        #[serde(default)]
        warnings: Vec<Warning>,
    },
    /// Transcript text increment.
    TranscriptDelta {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Appended text.
        delta: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A partial (interim) transcript.
    TranscriptPartial {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Interim text.
        text: String,
        /// Start time in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_second: Option<f64>,
        /// Duration in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_in_seconds: Option<f64>,
        /// Audio channel index.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_index: Option<u32>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A final transcript segment.
    TranscriptFinal {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Final text.
        text: String,
        /// Start time in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_second: Option<f64>,
        /// End time in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_second: Option<f64>,
        /// Audio channel index.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_index: Option<u32>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Response metadata.
    ResponseMetadata {
        /// Timestamp.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp: Option<DateTime<Utc>>,
        /// Model id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_id: Option<ModelId>,
        /// Headers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<Headers>,
        /// Body.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<JsonValue>,
    },
    /// Last part of a successful stream.
    Finish {
        /// Full transcript.
        text: String,
        /// Timed segments.
        #[serde(default)]
        segments: Vec<TranscriptionSegment>,
        /// Detected language.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// Audio duration in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_in_seconds: Option<f64>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// A raw provider chunk.
    Raw {
        /// The chunk as JSON.
        raw_value: JsonValue,
    },
    /// An error; the stream ends after this part.
    Error {
        /// The error.
        error: StreamError,
    },
}

/// Result of a streaming transcription call.
pub struct TranscriptionStreamResult {
    /// The stream of parts.
    pub stream: BoxStream<'static, TranscriptionStreamPart>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata known at stream start.
    pub response: ResponseMetadata,
}

impl std::fmt::Debug for TranscriptionStreamResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionStreamResult")
            .field("stream", &"<stream>")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish()
    }
}
