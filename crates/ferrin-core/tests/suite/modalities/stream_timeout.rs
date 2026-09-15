use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::stream_speech_translation;
use ferrin_core::stream_transcribe;
use ferrin_core::timeout::TimeoutScope;
use ferrin_spec::AudioFormat;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::SpeechTranslationModel;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamResult;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionResult;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamPart;
use ferrin_spec::transcription_model::TranscriptionStreamResult;
use futures_util::StreamExt;
use futures_util::stream;
use pretty_assertions::assert_eq;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

struct HungAudio {
    provider: ProviderId,
    model: ModelId,
    hang_open: bool,
    cancellation: Mutex<Option<CancellationToken>>,
}

impl HungAudio {
    fn new(hang_open: bool) -> Arc<Self> {
        Arc::new(Self {
            provider: ProviderId::new("mock"),
            model: ModelId::new("audio"),
            hang_open,
            cancellation: Mutex::new(None),
        })
    }

    async fn opened(&self, token: CancellationToken) {
        *self.cancellation.lock().unwrap() = Some(token);
        if self.hang_open {
            std::future::pending::<()>().await;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    }
}

impl TranscriptionModel for HungAudio {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model
    }
    fn supports_stream(&self) -> bool {
        true
    }
    async fn do_generate(
        &self,
        _: TranscriptionOptions,
    ) -> Result<TranscriptionResult, ProviderError> {
        Err(ProviderError::unsupported("non-streaming"))
    }
    async fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> Result<TranscriptionStreamResult, ProviderError> {
        self.opened(options.cancellation).await;
        Ok(TranscriptionStreamResult {
            stream: Box::pin(
                stream::iter([TranscriptionStreamPart::StreamStart {
                    warnings: Vec::new(),
                }])
                .chain(stream::pending()),
            ),
            request: Default::default(),
            response: Default::default(),
        })
    }
}

impl SpeechTranslationModel for HungAudio {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model
    }
    async fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> Result<SpeechTranslationStreamResult, ProviderError> {
        self.opened(options.cancellation).await;
        Ok(SpeechTranslationStreamResult {
            stream: Box::pin(
                stream::iter([SpeechTranslationStreamPart::StreamStart {
                    warnings: Vec::new(),
                }])
                .chain(stream::pending()),
            ),
            request: Default::default(),
            response: Default::default(),
        })
    }
}

fn assert_timeout(error: Error) {
    assert!(
        matches!(
            error,
            Error::Timeout {
                scope: TimeoutScope::Total,
                ..
            }
        ),
        "{error}"
    );
}

#[tokio::test(start_paused = true)]
async fn stream_timeout_bounds_transcription_and_translation_establishment() {
    let duration = Duration::from_secs(5);
    let model = HungAudio::new(true);
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        stream_transcribe(
            Arc::clone(&model),
            stream::empty(),
            AudioFormat::new("pcm16"),
        )
        .timeout(duration),
    )
    .await
    .unwrap();
    assert_timeout(result.unwrap_err());
    assert_eq!((start.elapsed(), model.is_cancelled()), (duration, true));

    let model = HungAudio::new(true);
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        stream_speech_translation(
            Arc::clone(&model),
            stream::empty(),
            AudioFormat::new("pcm16"),
            "en",
        )
        .timeout(duration),
    )
    .await
    .unwrap();
    assert_timeout(result.unwrap_err());
    assert_eq!((start.elapsed(), model.is_cancelled()), (duration, true));
}

#[tokio::test(start_paused = true)]
async fn stream_timeout_uses_one_deadline_and_emits_one_terminal_error() {
    let duration = Duration::from_secs(5);
    let model = HungAudio::new(false);
    let start = Instant::now();
    let result = stream_transcribe(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
    )
    .timeout(duration)
    .await
    .unwrap();
    tokio::time::advance(Duration::from_secs(2)).await;
    let parts = tokio::time::timeout(
        Duration::from_secs(10),
        result.into_parts().collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    assert_eq!(
        (parts.len(), start.elapsed(), model.is_cancelled()),
        (2, duration, true)
    );
    match &parts[1] {
        TranscriptionStreamPart::Error { error } => {
            assert_eq!(error.error_type.as_deref(), Some("timeout"))
        }
        other => panic!("unexpected part: {other:?}"),
    }

    let model = HungAudio::new(false);
    let start = Instant::now();
    let result = stream_speech_translation(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
        "en",
    )
    .timeout(duration)
    .await
    .unwrap();
    tokio::time::advance(Duration::from_secs(2)).await;
    let parts = tokio::time::timeout(Duration::from_secs(10), result.stream.collect::<Vec<_>>())
        .await
        .unwrap();
    assert_eq!(
        (parts.len(), start.elapsed(), model.is_cancelled()),
        (2, duration, true)
    );
    match &parts[1] {
        SpeechTranslationStreamPart::Error { error } => {
            assert_eq!(error.error_type.as_deref(), Some("timeout"))
        }
        other => panic!("unexpected part: {other:?}"),
    }
}

#[tokio::test]
async fn streaming_cancellation_and_drop_cancel_only_provider_work() {
    let model = HungAudio::new(false);
    let token = CancellationToken::new();
    let result = stream_transcribe(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
    )
    .cancellation(token.clone())
    .await
    .unwrap();
    drop(result);
    assert_eq!((model.is_cancelled(), token.is_cancelled()), (true, false));

    let model = HungAudio::new(false);
    let result = stream_speech_translation(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
        "en",
    )
    .cancellation(token.clone())
    .await
    .unwrap();
    token.cancel();
    let parts = result.stream.collect::<Vec<_>>().await;
    assert_eq!(parts.len(), 1);
    match &parts[0] {
        SpeechTranslationStreamPart::Error { error } => {
            assert_eq!(error.error_type.as_deref(), Some("cancelled"))
        }
        other => panic!("unexpected part: {other:?}"),
    }
    assert!(model.is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn maximum_stream_timeout_does_not_overflow_the_clock() {
    let model = HungAudio::new(false);
    let mut result = stream_transcribe(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
    )
    .timeout(Duration::MAX)
    .await
    .unwrap();
    assert!(matches!(
        result.parts().next().await,
        Some(TranscriptionStreamPart::StreamStart { .. })
    ));
    assert!(!model.is_cancelled());
    drop(result);
    assert!(model.is_cancelled());

    let model = HungAudio::new(false);
    let mut result = stream_speech_translation(
        Arc::clone(&model),
        stream::empty(),
        AudioFormat::new("pcm16"),
        "en",
    )
    .timeout(Duration::MAX)
    .await
    .unwrap();
    assert!(matches!(
        result.stream.next().await,
        Some(SpeechTranslationStreamPart::StreamStart { .. })
    ));
    assert!(!model.is_cancelled());
    drop(result);
    assert!(model.is_cancelled());
}
