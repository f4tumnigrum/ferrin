use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_core::Error;
use ferrin_core::generate_speech;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::SpeechModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_model::SpeechOptions;
use ferrin_spec::speech_model::SpeechResult;
use pretty_assertions::assert_eq;

use super::common::lock;
use super::common::media;

struct SpeechMock {
    provider: ProviderId,
    model_id: ModelId,
    audio: Bytes,
    media_type: Option<&'static str>,
    calls: Mutex<Vec<SpeechOptions>>,
}

impl SpeechModel for SpeechMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn do_generate(
        &self,
        options: SpeechOptions,
    ) -> impl Future<Output = Result<SpeechResult, ProviderError>> + Send {
        lock(&self.calls).push(options);
        let audio = self.audio.clone();
        let media_type = self.media_type.map(media);
        async move {
            Ok(SpeechResult {
                audio,
                media_type,
                warnings: Vec::new(),
                request: RequestMetadata::default(),
                response: ResponseMetadata::default(),
                provider_metadata: None,
            })
        }
    }
}

fn mock(audio: &'static [u8], media_type: Option<&'static str>) -> Arc<SpeechMock> {
    Arc::new(SpeechMock {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("speech-mock"),
        audio: Bytes::from_static(audio),
        media_type,
        calls: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn generates_speech_and_derives_the_format() {
    let model = mock(b"RIFF....WAVEfmt ", Some("audio/ogg"));
    let result = generate_speech(Arc::clone(&model), "Hello")
        .voice("alloy")
        .speed(1.25)
        .language("en")
        .await
        .unwrap();
    assert_eq!(result.audio.media_type.as_str(), "audio/wav");
    assert_eq!(result.audio.format, "wav");
    assert_eq!(result.audio.data.len(), 16);
    let calls = lock(&model.calls);
    assert_eq!(calls[0].text, "Hello");
    assert_eq!(calls[0].voice.as_deref(), Some("alloy"));
    assert_eq!(calls[0].speed, Some(1.25));
    assert_eq!(calls[0].language.as_deref(), Some("en"));
}

#[tokio::test]
async fn unknown_media_type_defaults_to_mp3() {
    let model = mock(b"\x00\x01\x02\x03", Some("audio/wav"));
    let result = generate_speech(model, "Hello").await.unwrap();
    assert_eq!(result.audio.media_type.as_str(), "audio/mp3");
    assert_eq!(result.audio.format, "mp3");
}

#[tokio::test]
async fn empty_audio_fails() {
    let model = mock(b"", Some("audio/wav"));
    let error = generate_speech(model, "Hello").await.unwrap_err();
    assert!(matches!(error, Error::NoSpeechGenerated { .. }), "{error}");
}
