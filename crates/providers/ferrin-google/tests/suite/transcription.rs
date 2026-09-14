//! Transcription model (Interactions API).

use bytes::Bytes;
use ferrin_spec::error::ProviderError;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionSegment;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::google_options;

#[tokio::test]
async fn generate_posts_an_interaction_and_maps_word_timestamps() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/interactions",
        "transcription",
        "generate",
    );
    let model = test.provider.transcription("gemini-3.1-flash-preview");
    assert!(!model.supports_stream());
    let mut options = TranscriptionOptions::new(Bytes::from_static(b"RIFFaudio"), "audio/wav");
    options.provider_options = google_options(json!({
        "languageCodes": ["en-US"],
        "customVocabulary": ["Ferrin"],
        "wordTimestamp": true,
        "diarization": true,
        "mode": "SMART"
    }));
    let result = model.do_generate(options).await.unwrap();
    assert_eq!(result.text, "Hello world.");
    assert_eq!(
        result.segments,
        vec![
            TranscriptionSegment {
                text: "Hello".to_owned(),
                start_second: 0.1,
                end_second: 0.5,
            },
            TranscriptionSegment {
                text: "world.".to_owned(),
                start_second: 0.6,
                end_second: 1.2,
            },
        ]
    );
    assert!(result.language.is_none());
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["google"]["usage"]["input_tokens"], json!(120));
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("transcription_request", request);
}

#[tokio::test]
async fn gemini_live_api_models_are_rejected() {
    let test = TestProvider::start().await;
    let error = test
        .provider
        .transcription("gemini-2.5-flash-live")
        .do_generate(TranscriptionOptions::new(
            Bytes::from_static(b"a"),
            "audio/wav",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
