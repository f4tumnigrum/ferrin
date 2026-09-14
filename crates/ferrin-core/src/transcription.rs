//! Transcription: [`transcribe`] for complete audio (bytes or URL) and
//! [`stream_transcribe`] for live audio.
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §4.

use std::fmt;
use std::future::IntoFuture;
use std::sync::Arc;

use bytes::Bytes;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_spec::AudioFormat;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::transcription_model::TranscriptionOptions;
pub use ferrin_spec::transcription_model::TranscriptionSegment;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
pub use ferrin_spec::transcription_model::TranscriptionStreamPart;
use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::stream;
use tracing::Instrument;
use url::Url;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::prompt::DefaultDownloader;
use crate::prompt::DownloadFn;
use crate::prompt::DownloadRequest;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::retry;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Media type used when detection fails.
const DEFAULT_AUDIO_MEDIA_TYPE: &str = "audio/wav";

/// Audio to transcribe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioInput {
    /// Audio bytes.
    Bytes(Bytes),
    /// A URL fetched with the download function.
    Url(Url),
}

impl From<Bytes> for AudioInput {
    fn from(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<Vec<u8>> for AudioInput {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(Bytes::from(bytes))
    }
}

impl From<Url> for AudioInput {
    fn from(url: Url) -> Self {
        Self::Url(url)
    }
}

/// Result of [`transcribe`] and of [`StreamTranscribeResult::consume`].
#[derive(Debug, Clone, PartialEq)]
pub struct TranscribeResult {
    /// The transcript.
    pub text: String,
    /// Timed segments.
    pub segments: Vec<TranscriptionSegment>,
    /// Detected language.
    pub language: Option<String>,
    /// Audio duration in seconds.
    pub duration_in_seconds: Option<f64>,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata of the calls made.
    pub responses: Vec<ResponseMetadata>,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Transcribes complete audio.
#[must_use]
pub fn transcribe(
    model: impl Into<TranscriptionModelRef>,
    audio: impl Into<AudioInput>,
) -> Transcribe {
    Transcribe {
        model: model.into(),
        audio: audio.into(),
        media_type: None,
        download: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`transcribe`]; `.await` runs the call.
pub struct Transcribe {
    model: TranscriptionModelRef,
    audio: AudioInput,
    media_type: Option<MediaType>,
    download: Option<Arc<dyn DownloadFn>>,
    base: ModalityOptions,
}

impl fmt::Debug for Transcribe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transcribe")
            .field("model", &self.model)
            .field("audio", &self.audio)
            .field("media_type", &self.media_type)
            .field("has_download", &self.download.is_some())
            .field("base", &self.base)
            .finish()
    }
}

impl Transcribe {
    /// Overrides the detected media type of the audio.
    #[must_use]
    pub fn media_type(mut self, media_type: impl Into<MediaType>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the function used to fetch [`AudioInput::Url`].
    #[must_use]
    pub fn download(mut self, download: Arc<dyn DownloadFn>) -> Self {
        self.download = Some(download);
        self
    }
}

impl_modality_builder!(Transcribe);

impl IntoFuture for Transcribe {
    type Output = Result<TranscribeResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(self))
    }
}

/// Fetches audio given as a URL.
async fn fetch_audio(
    input: AudioInput,
    download: Option<Arc<dyn DownloadFn>>,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<(Bytes, Option<MediaType>), Error> {
    match input {
        AudioInput::Bytes(bytes) => Ok((bytes, None)),
        AudioInput::Url(url) => {
            let downloader: Arc<dyn DownloadFn> = match download {
                Some(download) => download,
                None => Arc::new(DefaultDownloader::try_default()?),
            };
            let mut downloaded = downloader
                .download(
                    vec![DownloadRequest {
                        url: url.clone(),
                        is_url_supported_by_model: false,
                    }],
                    cancellation.clone(),
                )
                .await?;
            match downloaded.pop().flatten() {
                Some(file) => Ok((file.data, file.media_type)),
                None => Err(Error::download(
                    url,
                    None,
                    Some("the download function returned no data".into()),
                )),
            }
        }
    }
}

async fn run(builder: Transcribe) -> Result<TranscribeResult, Error> {
    let model = resolve_model(&builder.model, ProviderRegistry::transcription_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("transcription", &identity);
    let base = builder.base.clone();
    base.run(|base, token| {
        async move {
            let (audio, downloaded_media_type) =
                fetch_audio(builder.audio, builder.download, &token).await?;
            let media_type = builder
                .media_type
                .or(downloaded_media_type)
                .or_else(|| detect_media_type_for(&audio, "audio"))
                .unwrap_or_else(|| MediaType::new(DEFAULT_AUDIO_MEDIA_TYPE));
            let headers = base.request_headers();
            let result = retry(&base.retry_policy, &token, |_| {
                let options = TranscriptionOptions {
                    audio: audio.clone(),
                    media_type: media_type.clone(),
                    provider_options: base.provider_options.clone(),
                    headers: headers.clone(),
                    cancellation: token.child_token(),
                };
                let model = &model;
                async move { model.do_generate(options).await.map_err(Error::from) }
            })
            .await?;
            spans::log_warnings(&result.warnings, &identity);
            if result.text.is_empty() {
                return Err(Error::NoTranscriptGenerated {
                    responses: vec![result.response],
                });
            }
            Ok(TranscribeResult {
                text: result.text,
                segments: result.segments,
                language: result.language,
                duration_in_seconds: result.duration_in_seconds,
                warnings: result.warnings,
                request: result.request,
                responses: vec![result.response],
                provider_metadata: result.provider_metadata,
            })
        }
        .instrument(span)
    })
    .await
}

/// Transcribes a live audio stream. The model must support streaming
/// transcription (`supports_stream`).
#[must_use]
pub fn stream_transcribe(
    model: impl Into<TranscriptionModelRef>,
    audio: impl Stream<Item = Bytes> + Send + 'static,
    input_audio_format: AudioFormat,
) -> StreamTranscribe {
    StreamTranscribe {
        model: model.into(),
        audio: Box::pin(audio),
        input_audio_format,
        include_raw_chunks: false,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`stream_transcribe`]; `.await` opens the stream.
pub struct StreamTranscribe {
    model: TranscriptionModelRef,
    audio: BoxStream<'static, Bytes>,
    input_audio_format: AudioFormat,
    include_raw_chunks: bool,
    base: ModalityOptions,
}

impl fmt::Debug for StreamTranscribe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTranscribe")
            .field("model", &self.model)
            .field("input_audio_format", &self.input_audio_format)
            .field("include_raw_chunks", &self.include_raw_chunks)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl StreamTranscribe {
    /// Forwards raw provider chunks as [`TranscriptionStreamPart::Raw`].
    #[must_use]
    pub fn include_raw_chunks(mut self) -> Self {
        self.include_raw_chunks = true;
        self
    }
}

impl_modality_builder!(@no_retry StreamTranscribe);

impl IntoFuture for StreamTranscribe {
    type Output = Result<StreamTranscribeResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let model = resolve_model(&self.model, ProviderRegistry::transcription_model)?;
            let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
            if !model.supports_stream() {
                return Err(Error::from(ProviderError::unsupported(format!(
                    "streaming transcription (model `{}` of provider `{}`)",
                    identity.model_id, identity.provider
                ))));
            }
            let cancellation = self.base.cancellation.child_token();
            let result = model
                .do_stream(TranscriptionStreamOptions {
                    audio: self.audio,
                    input_audio_format: self.input_audio_format,
                    provider_options: self.base.provider_options.clone(),
                    headers: self.base.request_headers(),
                    include_raw_chunks: self.include_raw_chunks,
                    cancellation,
                })
                .await
                .map_err(Error::from)?;
            let log_identity = identity.clone();
            let parts = result.stream.inspect(move |part| {
                if let TranscriptionStreamPart::StreamStart { warnings } = part {
                    spans::log_warnings(warnings, &log_identity);
                }
            });
            Ok(StreamTranscribeResult {
                request: result.request,
                response: result.response,
                parts: Box::pin(parts),
            })
        })
    }
}

/// Result of [`stream_transcribe`]: the part stream plus request and
/// response metadata known when the stream opened.
pub struct StreamTranscribeResult {
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata known at stream start.
    pub response: ResponseMetadata,
    parts: BoxStream<'static, TranscriptionStreamPart>,
}

impl fmt::Debug for StreamTranscribeResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTranscribeResult")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish_non_exhaustive()
    }
}

impl StreamTranscribeResult {
    /// The stream of provider parts.
    pub fn parts(&mut self) -> &mut BoxStream<'static, TranscriptionStreamPart> {
        &mut self.parts
    }

    /// Takes the stream of provider parts.
    #[must_use]
    pub fn into_parts(self) -> BoxStream<'static, TranscriptionStreamPart> {
        self.parts
    }

    /// Transcript deltas; error parts end the stream with [`Error::Stream`].
    pub fn text_stream(self) -> impl Stream<Item = Result<String, Error>> + Send {
        self.parts
            .map(|part| match part {
                TranscriptionStreamPart::TranscriptDelta { delta, .. } => Some(Ok(delta)),
                TranscriptionStreamPart::Error { error } => Some(Err(Error::stream(error))),
                _ => None,
            })
            .filter_map(std::future::ready)
    }

    /// Drains the stream and returns the final transcript.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Stream`] when the provider emitted an error part
    /// and [`Error::NoTranscriptGenerated`] when the stream ended without
    /// a non-empty transcript.
    pub async fn consume(mut self) -> Result<TranscribeResult, Error> {
        let mut warnings: Vec<Warning> = Vec::new();
        let mut response = self.response.clone();
        while let Some(part) = self.parts.next().await {
            match part {
                TranscriptionStreamPart::StreamStart { warnings: started } => {
                    warnings.extend(started)
                }
                TranscriptionStreamPart::ResponseMetadata {
                    timestamp,
                    model_id,
                    headers,
                    body,
                } => {
                    if timestamp.is_some() {
                        response.timestamp = timestamp;
                    }
                    if model_id.is_some() {
                        response.model_id = model_id;
                    }
                    if headers.is_some() {
                        response.headers = headers;
                    }
                    if body.is_some() {
                        response.body = body;
                    }
                }
                TranscriptionStreamPart::Finish {
                    text,
                    segments,
                    language,
                    duration_in_seconds,
                    provider_metadata,
                } => {
                    if text.is_empty() {
                        return Err(Error::NoTranscriptGenerated {
                            responses: vec![response],
                        });
                    }
                    return Ok(TranscribeResult {
                        text,
                        segments,
                        language,
                        duration_in_seconds,
                        warnings,
                        request: self.request,
                        responses: vec![response],
                        provider_metadata,
                    });
                }
                TranscriptionStreamPart::Error { error } => return Err(Error::stream(error)),
                #[allow(
                    unreachable_patterns,
                    reason = "TranscriptionStreamPart is non-exhaustive"
                )]
                _ => {}
            }
        }
        Err(Error::NoTranscriptGenerated {
            responses: vec![response],
        })
    }
}

/// Converts a static list of parts into a stream (used by adapters and
/// tests to build simple transcription streams).
#[must_use]
pub fn transcription_parts_stream(
    parts: Vec<TranscriptionStreamPart>,
) -> BoxStream<'static, TranscriptionStreamPart> {
    Box::pin(stream::iter(parts))
}
