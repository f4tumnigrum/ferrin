//! Speech translation (streaming only): [`stream_speech_translation`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §10.

use std::fmt;
use std::future::IntoFuture;

use bytes::Bytes;
use ferrin_spec::AudioFormat;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::Warning;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
pub use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
pub use ferrin_spec::speech_translation_model::SpeechTranslationUsage;
use futures_core::Stream;
use futures_util::StreamExt;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::modality_stream::StreamDeadline;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Translates a live audio stream into `target_language`.
#[must_use]
pub fn stream_speech_translation(
    model: impl Into<SpeechTranslationModelRef>,
    audio: impl Stream<Item = Bytes> + Send + 'static,
    input_audio_format: AudioFormat,
    target_language: impl Into<String>,
) -> StreamSpeechTranslation {
    StreamSpeechTranslation {
        model: model.into(),
        audio: Box::pin(audio),
        input_audio_format,
        target_language: target_language.into(),
        source_language: None,
        output_audio_format: None,
        include_raw_chunks: false,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`stream_speech_translation`]; `.await` opens the
/// stream.
pub struct StreamSpeechTranslation {
    model: SpeechTranslationModelRef,
    audio: BoxStream<'static, Bytes>,
    input_audio_format: AudioFormat,
    target_language: String,
    source_language: Option<String>,
    output_audio_format: Option<AudioFormat>,
    include_raw_chunks: bool,
    base: ModalityOptions,
}

impl fmt::Debug for StreamSpeechTranslation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamSpeechTranslation")
            .field("model", &self.model)
            .field("input_audio_format", &self.input_audio_format)
            .field("target_language", &self.target_language)
            .field("source_language", &self.source_language)
            .field("output_audio_format", &self.output_audio_format)
            .field("include_raw_chunks", &self.include_raw_chunks)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl StreamSpeechTranslation {
    /// Language of the input audio (detected when unset).
    #[must_use]
    pub fn source_language(mut self, language: impl Into<String>) -> Self {
        self.source_language = Some(language.into());
        self
    }

    /// Requests translated audio in this format.
    #[must_use]
    pub fn output_audio_format(mut self, format: AudioFormat) -> Self {
        self.output_audio_format = Some(format);
        self
    }

    /// Forwards raw provider chunks.
    #[must_use]
    pub fn include_raw_chunks(mut self) -> Self {
        self.include_raw_chunks = true;
        self
    }
}

impl_modality_builder!(@no_retry StreamSpeechTranslation);

impl IntoFuture for StreamSpeechTranslation {
    type Output = Result<SpeechTranslationStreamResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let model = resolve_model(&self.model, ProviderRegistry::speech_translation_model)?;
            let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
            if self.target_language.trim().is_empty() {
                return Err(Error::invalid_argument(
                    "target_language",
                    "must not be empty",
                ));
            }
            let started_at = chrono::Utc::now();
            let deadline = StreamDeadline::new(&self.base.cancellation, self.base.timeout);
            let result = deadline
                .run(async {
                    model
                        .do_stream(SpeechTranslationStreamOptions {
                            audio: self.audio,
                            input_audio_format: self.input_audio_format,
                            target_language: self.target_language,
                            source_language: self.source_language,
                            output_audio_format: self.output_audio_format,
                            provider_options: self.base.provider_options.clone(),
                            headers: self.base.request_headers(),
                            include_raw_chunks: self.include_raw_chunks,
                            cancellation: deadline.cancellation.clone(),
                        })
                        .await
                        .map_err(Error::from)
                })
                .await?;
            let mut response = result.response;
            response.timestamp.get_or_insert(started_at);
            response
                .model_id
                .get_or_insert_with(|| identity.model_id.clone());
            let stream = result.stream.inspect(move |part| {
                if let SpeechTranslationStreamPart::StreamStart { warnings } = part {
                    spans::log_warnings(warnings, &identity);
                }
            });
            Ok(SpeechTranslationStreamResult {
                stream: deadline.wrap(
                    Box::pin(stream),
                    |error| SpeechTranslationStreamPart::Error { error },
                    |part| {
                        matches!(
                            part,
                            SpeechTranslationStreamPart::Finish { .. }
                                | SpeechTranslationStreamPart::Error { .. }
                        )
                    },
                ),
                request: result.request,
                response,
            })
        })
    }
}

/// Final speech translation collected by [`SpeechTranslationStreamResult::consume`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechTranslationResult {
    /// Complete transcript in the source language.
    pub source_text: String,
    /// Complete translated text; audio-only output may leave this empty.
    pub translation_text: String,
    /// Audio duration in seconds.
    pub duration_in_seconds: Option<f64>,
    /// Provider-reported token usage.
    pub usage: Option<SpeechTranslationUsage>,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Final response metadata.
    pub response: ResponseMetadata,
    /// Provider-specific metadata, empty when absent.
    pub provider_metadata: ProviderMetadata,
}

/// Owned speech translation stream with final-result collection.
pub struct SpeechTranslationStreamResult {
    /// Provider parts; one consumer owns the live stream.
    pub stream: BoxStream<'static, SpeechTranslationStreamPart>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata known at stream start.
    pub response: ResponseMetadata,
}

impl fmt::Debug for SpeechTranslationStreamResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpeechTranslationStreamResult")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish_non_exhaustive()
    }
}

impl SpeechTranslationStreamResult {
    /// Drains the owned stream and collects the final translation and metadata.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Stream`] for provider errors and
    /// [`Error::NoTranslationGenerated`] when no finish part arrives or the
    /// finished stream contains neither translated text nor an audio part.
    pub async fn consume(mut self) -> Result<SpeechTranslationResult, Error> {
        let mut warnings = Vec::new();
        let mut has_audio = false;
        while let Some(part) = self.stream.next().await {
            match part {
                SpeechTranslationStreamPart::StreamStart { warnings: started } => {
                    warnings.extend(started);
                }
                SpeechTranslationStreamPart::ResponseMetadata {
                    timestamp,
                    model_id,
                    headers,
                    body,
                } => {
                    if timestamp.is_some() {
                        self.response.timestamp = timestamp;
                    }
                    if model_id.is_some() {
                        self.response.model_id = model_id;
                    }
                    if headers.is_some() {
                        self.response.headers = headers;
                    }
                    if body.is_some() {
                        self.response.body = body;
                    }
                }
                SpeechTranslationStreamPart::Audio { .. } => has_audio = true,
                SpeechTranslationStreamPart::Finish {
                    source_text,
                    output_text,
                    duration_in_seconds,
                    usage,
                    provider_metadata,
                } => {
                    if !has_audio && output_text.is_empty() {
                        return Err(Error::NoTranslationGenerated {
                            response: Box::new(self.response),
                        });
                    }
                    return Ok(SpeechTranslationResult {
                        source_text,
                        translation_text: output_text,
                        duration_in_seconds,
                        usage,
                        warnings,
                        request: self.request,
                        response: self.response,
                        provider_metadata: provider_metadata.unwrap_or_default(),
                    });
                }
                SpeechTranslationStreamPart::Error { error } => return Err(Error::stream(error)),
                _ => {}
            }
        }
        Err(Error::NoTranslationGenerated {
            response: Box::new(self.response),
        })
    }
}
