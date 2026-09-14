//! Speech translation (streaming only): [`stream_speech_translation`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §10.

use std::fmt;
use std::future::IntoFuture;

use bytes::Bytes;
use ferrin_spec::AudioFormat;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::SpeechTranslationModelRef;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
pub use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
pub use ferrin_spec::speech_translation_model::SpeechTranslationStreamResult;
pub use ferrin_spec::speech_translation_model::SpeechTranslationUsage;
use futures_core::Stream;
use futures_util::StreamExt;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
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
            let result = model
                .do_stream(SpeechTranslationStreamOptions {
                    audio: self.audio,
                    input_audio_format: self.input_audio_format,
                    target_language: self.target_language,
                    source_language: self.source_language,
                    output_audio_format: self.output_audio_format,
                    provider_options: self.base.provider_options.clone(),
                    headers: self.base.request_headers(),
                    include_raw_chunks: self.include_raw_chunks,
                    cancellation: self.base.cancellation.child_token(),
                })
                .await
                .map_err(|error: ProviderError| Error::from(error))?;
            let stream = result.stream.inspect(move |part| {
                if let SpeechTranslationStreamPart::StreamStart { warnings } = part {
                    spans::log_warnings(warnings, &identity);
                }
            });
            Ok(SpeechTranslationStreamResult {
                stream: Box::pin(stream),
                request: result.request,
                response: result.response,
            })
        })
    }
}
