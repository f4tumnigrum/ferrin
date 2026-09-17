use std::sync::Arc;

use bytes::Bytes;
use ferrin_core::Error;
use ferrin_core::speech_translation::SpeechTranslationResult;
use ferrin_core::stream_speech_translation;
use ferrin_spec::AudioFormat;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::SpeechTranslationModel;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart as Part;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamResult;
use ferrin_spec::speech_translation_model::SpeechTranslationUsage;
use futures_util::stream;
use pretty_assertions::assert_eq;
use serde_json::json;

struct TranslationMock {
    provider: ProviderId,
    model: ModelId,
    parts: Vec<Part>,
}

impl SpeechTranslationModel for TranslationMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model
    }
    async fn do_stream(
        &self,
        _options: SpeechTranslationStreamOptions,
    ) -> Result<SpeechTranslationStreamResult, ProviderError> {
        Ok(SpeechTranslationStreamResult {
            stream: Box::pin(stream::iter(self.parts.clone())),
            request: RequestMetadata::default(),
            response: ResponseMetadata::default(),
        })
    }
}

async fn open(parts: Vec<Part>) -> ferrin_core::speech_translation::SpeechTranslationStreamResult {
    stream_speech_translation(
        Arc::new(TranslationMock {
            provider: "mock".into(),
            model: "translation".into(),
            parts,
        }),
        stream::empty::<Bytes>(),
        AudioFormat::new("pcm16"),
        "es",
    )
    .await
    .unwrap()
}

fn finish(output: &str) -> Part {
    Part::Finish {
        source_text: "Hello".into(),
        output_text: output.into(),
        duration_in_seconds: Some(0.5),
        usage: Some(SpeechTranslationUsage {
            input_audio_tokens: Some(3),
            output_text_tokens: Some(2),
            ..SpeechTranslationUsage::default()
        }),
        provider_metadata: Some(serde_json::from_value(json!({"mock":{"language":"es"}})).unwrap()),
    }
}

#[tokio::test]
async fn collects_final_translation_and_merges_response_metadata() {
    let timestamp = chrono::Utc::now();
    let warnings = vec![Warning::other("translation warning")];
    let stream = open(vec![
        Part::StreamStart {
            warnings: warnings.clone(),
        },
        Part::ResponseMetadata {
            timestamp: Some(timestamp),
            model_id: Some("resolved".into()),
            headers: None,
            body: None,
        },
        Part::ResponseMetadata {
            timestamp: None,
            model_id: None,
            headers: None,
            body: Some(json!({"id":"response"})),
        },
        finish("Hola"),
    ])
    .await;
    assert!(stream.response.timestamp.is_some());
    assert_eq!(stream.response.model_id, Some("translation".into()));
    let result = stream.consume().await.unwrap();
    assert_eq!(
        result,
        SpeechTranslationResult {
            source_text: "Hello".into(),
            translation_text: "Hola".into(),
            duration_in_seconds: Some(0.5),
            usage: Some(SpeechTranslationUsage {
                input_audio_tokens: Some(3),
                output_text_tokens: Some(2),
                ..SpeechTranslationUsage::default()
            }),
            warnings,
            request: RequestMetadata::default(),
            response: ResponseMetadata {
                timestamp: Some(timestamp),
                model_id: Some("resolved".into()),
                body: Some(json!({"id":"response"})),
                ..ResponseMetadata::default()
            },
            provider_metadata: serde_json::from_value::<ProviderMetadata>(
                json!({"mock":{"language":"es"}})
            )
            .unwrap(),
        }
    );
}

#[tokio::test]
async fn audio_only_translation_succeeds_but_missing_or_empty_finish_fails() {
    let result = open(vec![
        Part::Audio {
            id: None,
            audio: Bytes::from_static(b"pcm"),
            provider_metadata: None,
        },
        finish(""),
    ])
    .await
    .consume()
    .await
    .unwrap();
    assert_eq!(result.translation_text, "");
    for parts in [
        vec![],
        vec![finish("")],
        vec![Part::Audio {
            id: None,
            audio: Bytes::from_static(b"pcm"),
            provider_metadata: None,
        }],
    ] {
        let error = open(parts).await.consume().await.unwrap_err();
        match error {
            Error::NoTranslationGenerated { response } => {
                assert!(response.timestamp.is_some());
                assert_eq!(response.model_id, Some("translation".into()));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
