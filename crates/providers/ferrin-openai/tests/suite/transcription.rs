//! Transcriptions API (multipart upload) and model gating.

use bytes::Bytes;
use ferrin_openai::transcription::is_realtime_model;
use ferrin_openai::transcription::language_code;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::transcription_model::TranscriptionOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[test]
fn language_names_map_to_iso_codes() {
    assert_eq!(language_code("english"), Some("en"));
    assert_eq!(language_code("Chinese"), Some("zh"));
    assert_eq!(language_code("klingon"), None);
}

#[test]
fn realtime_models_are_detected_by_id() {
    assert!(is_realtime_model("gpt-realtime-whisper"));
    assert!(is_realtime_model("gpt-realtime-whisper-2026-01-01"));
    assert!(!is_realtime_model("whisper-1"));
}

#[tokio::test]
async fn verbose_json_maps_segments_language_and_duration() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/audio/transcriptions",
        "transcription",
        "verbose",
    );
    let model = test.provider.transcription("whisper-1");
    assert!(!model.supports_stream());
    let mut options = TranscriptionOptions::new(Bytes::from_static(b"ID3audio"), "audio/mpeg");
    options.provider_options = openai_options(json!({
        "language": "en",
        "prompt": "Names: Ferrin",
        "temperature": 0.2,
        "timestampGranularities": ["segment", "word"]
    }));
    let result = model.do_generate(options).await.unwrap();
    assert_eq!(result.text, "Hello world.");
    assert_eq!(result.language.as_deref(), Some("en"));
    assert_eq!(result.duration_in_seconds, Some(2.5));
    assert_eq!(result.segments.len(), 2);
    assert_eq!(result.segments[1].text, " world.");
    assert_eq!(result.segments[1].start_second, 1.2);
    assert!(result.provider_metadata.is_none());

    let request = test.only_request();
    let content_type = request.header("content-type").unwrap();
    assert!(
        content_type.starts_with("multipart/form-data"),
        "{content_type}"
    );
    let body = request.body_text();
    for expected in [
        "name=\"model\"\r\n\r\nwhisper-1",
        "name=\"file\"; filename=\"audio.mp3\"",
        "name=\"response_format\"\r\n\r\nverbose_json",
        "name=\"language\"\r\n\r\nen",
        "name=\"prompt\"\r\n\r\nNames: Ferrin",
        "name=\"temperature\"\r\n\r\n0.2",
        "name=\"timestamp_granularities[]\"\r\n\r\nsegment",
        "name=\"timestamp_granularities[]\"\r\n\r\nword",
    ] {
        assert!(body.contains(expected), "missing {expected:?} in {body}");
    }
}

#[tokio::test]
async fn diarized_segments_land_in_provider_metadata() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/audio/transcriptions",
        "transcription",
        "diarized",
    );
    let result = test
        .provider
        .transcription("gpt-4o-transcribe-diarize")
        .do_generate(TranscriptionOptions::new(
            Bytes::from_static(b"RIFF"),
            "audio/wav",
        ))
        .await
        .unwrap();
    assert_eq!(result.segments.len(), 2);
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["segments"][0]["speaker"], json!("A"));
    assert_eq!(metadata["openai"]["segments"][1]["endSecond"], json!(3.0));
    let body = test.only_request().body_text();
    assert!(
        body.contains("name=\"response_format\"\r\n\r\ndiarized_json"),
        "{body}"
    );
    assert!(
        body.contains("name=\"chunking_strategy\"\r\n\r\nauto"),
        "{body}"
    );
}

#[tokio::test]
async fn words_are_used_when_segments_are_missing() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/audio/transcriptions",
        "transcription",
        "words",
    );
    let mut options = TranscriptionOptions::new(Bytes::from_static(b"RIFF"), "audio/wav");
    options.provider_options = openai_options(json!({
        "chunkingStrategy": {"type": "server_vad", "threshold": 0.5, "prefixPaddingMs": 300}
    }));
    let result = test
        .provider
        .transcription("gpt-4o-transcribe")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.language.as_deref(), Some("de"));
    assert_eq!(result.segments.len(), 2);
    assert_eq!(result.segments[0].text, "Hallo");
    let body = test.only_request().body_text();
    assert!(
        body.contains("name=\"response_format\"\r\n\r\njson"),
        "{body}"
    );
    assert!(
        body.contains("{\"type\":\"server_vad\",\"threshold\":0.5,\"prefix_padding_ms\":300}"),
        "{body}"
    );
}

#[tokio::test]
async fn realtime_model_rejects_non_streaming_transcription() {
    let test = TestProvider::start().await;
    let error = test
        .provider
        .transcription("gpt-realtime-whisper")
        .do_generate(TranscriptionOptions::new(
            Bytes::from_static(b"RIFF"),
            "audio/wav",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
