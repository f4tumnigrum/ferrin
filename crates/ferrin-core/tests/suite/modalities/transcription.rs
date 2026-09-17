use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_core::Error;
use ferrin_core::stream_transcribe;
use ferrin_core::transcribe;
use ferrin_core::transcription::TranscriptionStreamPart;
use ferrin_spec::AudioFormat;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionResult;
use ferrin_spec::transcription_model::TranscriptionSegment;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamResult;
use futures_util::StreamExt;
use futures_util::stream;
use pretty_assertions::assert_eq;

use super::common::lock;

struct TranscriptionMock {
    provider: ProviderId,
    model_id: ModelId,
    text: &'static str,
    streaming: bool,
    stream_parts: Vec<TranscriptionStreamPart>,
    calls: Mutex<Vec<TranscriptionOptions>>,
}

impl TranscriptionModel for TranscriptionMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> impl Future<Output = Result<TranscriptionResult, ProviderError>> + Send {
        lock(&self.calls).push(options);
        let text = self.text.to_owned();
        async move {
            Ok(TranscriptionResult {
                text: text.clone(),
                segments: vec![TranscriptionSegment {
                    text,
                    start_second: 0.0,
                    end_second: 1.5,
                }],
                language: Some("en".to_owned()),
                duration_in_seconds: Some(1.5),
                warnings: Vec::new(),
                request: RequestMetadata::default(),
                response: ResponseMetadata::default(),
                provider_metadata: None,
            })
        }
    }

    fn supports_stream(&self) -> bool {
        self.streaming
    }

    fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> impl Future<Output = Result<TranscriptionStreamResult, ProviderError>> + Send {
        let parts = self.stream_parts.clone();
        async move {
            // Drain the audio so the producer side completes.
            let _chunks: Vec<Bytes> = options.audio.collect().await;
            Ok(TranscriptionStreamResult {
                stream: Box::pin(stream::iter(parts)),
                request: RequestMetadata::default(),
                response: ResponseMetadata::default(),
            })
        }
    }
}

fn mock(text: &'static str) -> Arc<TranscriptionMock> {
    Arc::new(TranscriptionMock {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("transcription-mock"),
        text,
        streaming: false,
        stream_parts: Vec::new(),
        calls: Mutex::new(Vec::new()),
    })
}

fn streaming_mock(parts: Vec<TranscriptionStreamPart>) -> Arc<TranscriptionMock> {
    Arc::new(TranscriptionMock {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("transcription-mock"),
        text: "",
        streaming: true,
        stream_parts: parts,
        calls: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn transcribes_bytes_and_detects_the_media_type() {
    let model = mock("hello world");
    let result = transcribe(Arc::clone(&model), Bytes::from_static(b"RIFF....WAVEfmt "))
        .await
        .unwrap();
    assert_eq!(result.text, "hello world");
    assert_eq!(result.segments.len(), 1);
    assert_eq!(result.language.as_deref(), Some("en"));
    let calls = lock(&model.calls);
    assert_eq!(calls[0].media_type.as_str(), "audio/wav");
}

#[tokio::test]
async fn media_type_override_and_default() {
    let model = mock("x");
    transcribe(Arc::clone(&model), vec![0u8, 1, 2, 3])
        .media_type("audio/ogg")
        .await
        .unwrap();
    transcribe(Arc::clone(&model), vec![0u8, 1, 2, 3])
        .await
        .unwrap();
    let calls = lock(&model.calls);
    assert_eq!(calls[0].media_type.as_str(), "audio/ogg");
    assert_eq!(calls[1].media_type.as_str(), "audio/wav");
}

#[tokio::test]
async fn empty_transcript_fails() {
    let error = transcribe(mock(""), vec![0u8; 4]).await.unwrap_err();
    assert!(
        matches!(error, Error::NoTranscriptGenerated { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn streaming_requires_support() {
    let error = stream_transcribe(
        mock("x"),
        stream::iter(vec![Bytes::from_static(b"a")]),
        AudioFormat::new("pcm16"),
    )
    .await
    .unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");
}

#[tokio::test]
async fn streaming_yields_deltas_and_a_final_transcript() {
    let parts = vec![
        TranscriptionStreamPart::StreamStart {
            warnings: Vec::new(),
        },
        TranscriptionStreamPart::TranscriptDelta {
            id: None,
            delta: "Hel".to_owned(),
            provider_metadata: None,
        },
        TranscriptionStreamPart::TranscriptDelta {
            id: None,
            delta: "lo".to_owned(),
            provider_metadata: None,
        },
        TranscriptionStreamPart::Finish {
            text: "Hello".to_owned(),
            segments: Vec::new(),
            language: None,
            duration_in_seconds: Some(0.5),
            provider_metadata: None,
        },
    ];
    let model = streaming_mock(parts.clone());
    let deltas: Vec<String> = stream_transcribe(
        Arc::clone(&model),
        stream::iter(vec![Bytes::from_static(b"a")]),
        AudioFormat::new("pcm16"),
    )
    .await
    .unwrap()
    .text_stream()
    .map(Result::unwrap)
    .collect()
    .await;
    assert_eq!(deltas, vec!["Hel", "lo"]);

    let result = stream_transcribe(
        model,
        stream::iter(vec![Bytes::from_static(b"a")]),
        AudioFormat::new("pcm16"),
    )
    .await
    .unwrap()
    .consume()
    .await
    .unwrap();
    assert_eq!(result.text, "Hello");
    assert_eq!(result.duration_in_seconds, Some(0.5));
}

#[tokio::test]
async fn streams_without_a_finish_part_report_no_transcript() {
    let model = streaming_mock(vec![TranscriptionStreamPart::StreamStart {
        warnings: Vec::new(),
    }]);
    let error = stream_transcribe(
        model,
        stream::iter(Vec::<Bytes>::new()),
        AudioFormat::new("pcm16"),
    )
    .await
    .unwrap()
    .consume()
    .await
    .unwrap_err();
    assert!(
        matches!(error, Error::NoTranscriptGenerated { .. }),
        "{error}"
    );
}

struct MislabelledDownload;

impl ferrin_core::prompt::DownloadFn for MislabelledDownload {
    fn download(
        &self,
        _requests: Vec<ferrin_core::prompt::DownloadRequest>,
        _cancellation: tokio_util::sync::CancellationToken,
    ) -> ferrin_spec::BoxFuture<'_, Result<Vec<Option<ferrin_core::prompt::DownloadedFile>>, Error>>
    {
        Box::pin(async {
            Ok(vec![Some(ferrin_core::prompt::DownloadedFile {
                data: Bytes::from_static(b"RIFF....WAVEfmt "),
                media_type: Some("audio/ogg".into()),
            })])
        })
    }
}

#[tokio::test]
async fn downloaded_audio_is_detected_from_bytes_instead_of_http_media_type() {
    let model = mock("hello");
    transcribe(
        Arc::clone(&model),
        url::Url::parse("https://example.com/audio").unwrap(),
    )
    .download(Arc::new(MislabelledDownload))
    .await
    .unwrap();
    assert_eq!(lock(&model.calls)[0].media_type.as_str(), "audio/wav");
}

#[tokio::test]
async fn streaming_metadata_defaults_to_start_time_and_requested_model() {
    let started_at = chrono::Utc::now();
    let result = stream_transcribe(
        streaming_mock(vec![TranscriptionStreamPart::Finish {
            text: "Hello".into(),
            segments: Vec::new(),
            language: None,
            duration_in_seconds: None,
            provider_metadata: None,
        }]),
        stream::empty::<Bytes>(),
        AudioFormat::new("pcm16"),
    )
    .await
    .unwrap()
    .consume()
    .await
    .unwrap();
    assert!(
        result.responses[0]
            .timestamp
            .is_some_and(|timestamp| timestamp >= started_at)
    );
    assert_eq!(
        result.responses[0].model_id,
        Some("transcription-mock".into())
    );
}
