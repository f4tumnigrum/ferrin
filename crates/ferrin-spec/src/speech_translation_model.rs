//! Speech translation model interface (streaming only).

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
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;
use crate::shared::base64_bytes;

/// A model that translates streamed speech into text (and optionally audio).
pub trait SpeechTranslationModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Translates a live audio stream.
    fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> impl Future<Output = Result<SpeechTranslationStreamResult, ProviderError>> + Send;
}

/// Options for a speech translation stream.
pub struct SpeechTranslationStreamOptions {
    /// Audio chunks.
    pub audio: BoxStream<'static, Bytes>,
    /// Format of the audio chunks.
    pub input_audio_format: AudioFormat,
    /// Target language code.
    pub target_language: String,
    /// Source language code, if known.
    pub source_language: Option<String>,
    /// Requested output audio format, if audio output is wanted.
    pub output_audio_format: Option<AudioFormat>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Whether to include `Raw` parts.
    pub include_raw_chunks: bool,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl std::fmt::Debug for SpeechTranslationStreamOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechTranslationStreamOptions")
            .field("audio", &"<stream>")
            .field("input_audio_format", &self.input_audio_format)
            .field("target_language", &self.target_language)
            .field("source_language", &self.source_language)
            .field("output_audio_format", &self.output_audio_format)
            .field("provider_options", &self.provider_options)
            .field("headers", &self.headers)
            .field("include_raw_chunks", &self.include_raw_chunks)
            .finish_non_exhaustive()
    }
}

/// Usage of a speech translation stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SpeechTranslationUsage {
    /// Seconds of input audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_seconds: Option<f64>,
    /// Input audio tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_tokens: Option<u64>,
    /// Output audio tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_audio_tokens: Option<u64>,
    /// Input text tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_text_tokens: Option<u64>,
    /// Output text tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_text_tokens: Option<u64>,
}

/// A part of a speech translation stream, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SpeechTranslationStreamPart {
    /// First part of every stream.
    StreamStart {
        /// Warnings.
        #[serde(default)]
        warnings: Vec<Warning>,
    },
    /// Translated audio chunk.
    Audio {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Audio bytes.
        #[serde(with = "base64_bytes")]
        audio: Bytes,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Translated text increment.
    OutputTextDelta {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Appended text.
        delta: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Final translated text for a part.
    OutputTextFinal {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// The text.
        text: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Source transcript increment.
    SourceTranscriptDelta {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Appended text.
        delta: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    /// Interim source transcript.
    SourceTranscriptPartial {
        /// Part id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Interim text.
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
    /// Final source transcript segment.
    SourceTranscriptFinal {
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
        /// Full source transcript.
        source_text: String,
        /// Full translated text.
        output_text: String,
        /// Audio duration in seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_in_seconds: Option<f64>,
        /// Usage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<SpeechTranslationUsage>,
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

/// Result of a speech translation stream call.
pub struct SpeechTranslationStreamResult {
    /// The stream of parts.
    pub stream: BoxStream<'static, SpeechTranslationStreamPart>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata known at stream start.
    pub response: ResponseMetadata,
}

impl std::fmt::Debug for SpeechTranslationStreamResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechTranslationStreamResult")
            .field("stream", &"<stream>")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish()
    }
}
