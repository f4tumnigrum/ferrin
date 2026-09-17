//! Speech API.

use bytes::Bytes;
use ferrin_spec::SpeechModel;
use ferrin_spec::speech_model::SpeechOptions;
use ferrin_testing::Fixture;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[tokio::test]
async fn generate_returns_audio_bytes_and_media_type() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/audio/speech",
        Fixture::complete(
            StatusCode::OK,
            "audio/mpeg",
            Bytes::from_static(b"ID3fake-mp3"),
        ),
    );
    let mut options = SpeechOptions::new("Hello there");
    options.voice = Some("nova".to_owned());
    options.output_format = Some("mp3".to_owned());
    options.speed = Some(1.2);
    options.instructions = Some("Cheerful".to_owned());
    options.provider_options = openai_options(json!({"instructions": "Ignored", "speed": 2.0}));
    let result = test
        .provider
        .speech("gpt-4o-mini-tts")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.audio, Bytes::from_static(b"ID3fake-mp3"));
    assert_eq!(
        result
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("audio/mpeg")
    );
    assert!(result.warnings.is_empty());
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "gpt-4o-mini-tts",
            "input": "Hello there",
            "voice": "nova",
            "response_format": "mp3",
            "speed": 1.2,
            "instructions": "Cheerful"
        })
    );
}

#[tokio::test]
async fn unsupported_format_and_language_produce_warnings() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/audio/speech",
        Fixture::complete(StatusCode::OK, "audio/mpeg", Bytes::from_static(b"ID3")),
    );
    let mut options = SpeechOptions::new("Hello");
    options.output_format = Some("ogg".to_owned());
    options.language = Some("fr".to_owned());
    let result = test
        .provider
        .speech("tts-1")
        .do_generate(options)
        .await
        .unwrap();
    let features: Vec<&str> = result
        .warnings
        .iter()
        .filter_map(|warning| match warning {
            ferrin_spec::Warning::Unsupported { feature, .. } => Some(feature.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(features, vec!["outputFormat", "language"]);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["response_format"], json!("mp3"));
    assert_eq!(request["voice"], json!("alloy"));
}

#[tokio::test]
async fn provider_speech_options_are_validated_but_not_forwarded() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/audio/speech",
        Fixture::complete(StatusCode::OK, "audio/mpeg", Bytes::from_static(b"ID3")),
    );
    let mut options = SpeechOptions::new("hello");
    options.provider_options = openai_options(json!({"instructions":"ignored","speed":1.5}));
    test.provider
        .speech("tts-1")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(
        test.only_request().body_json().unwrap(),
        json!({"model":"tts-1","input":"hello","voice":"alloy","response_format":"mp3"})
    );
    let mut invalid = SpeechOptions::new("hello");
    invalid.provider_options = openai_options(json!({"speed":5.0}));
    assert!(matches!(
        test.provider
            .speech("tts-1")
            .do_generate(invalid)
            .await
            .unwrap_err(),
        ferrin_spec::error::ProviderError::InvalidArgument(_)
    ));
    assert_eq!(test.server.received_count(), 1);
}
