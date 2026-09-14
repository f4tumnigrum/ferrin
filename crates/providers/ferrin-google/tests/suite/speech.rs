//! Speech (text-to-speech) model.

use ferrin_spec::speech_model::SpeechModel;
use ferrin_spec::speech_model::SpeechOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::features;
use super::common::google_options;

const PATH: &str = "/v1beta/models/gemini-2.5-flash-preview-tts:generateContent";

#[tokio::test]
async fn generate_wraps_pcm_audio_in_a_wav_container() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, PATH, "speech", "generate");
    let mut options = SpeechOptions::new("Hello");
    options.instructions = Some("Cheerfully".to_owned());
    options.speed = Some(1.2);
    options.language = Some("en".to_owned());
    let result = test
        .provider
        .speech("gemini-2.5-flash-preview-tts")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(features(&result.warnings), vec!["speed", "language"]);
    assert_eq!(
        result
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("audio/wav")
    );
    assert_eq!(result.audio.len(), 44 + 8);
    assert_eq!(&result.audio[..4], b"RIFF");
    assert_eq!(&result.audio[8..12], b"WAVE");
    assert_eq!(&result.audio[44..], b"\x00\x00\x01\x00\x02\x00\x03\x00");
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["google"]["sampleRate"], json!(24_000));
    assert_eq!(
        metadata["google"]["mimeType"],
        json!("audio/L16;codec=pcm;rate=24000")
    );
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("speech_request", request);
}

#[tokio::test]
async fn pcm_output_and_multi_speaker_config_are_passed_through() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, PATH, "speech", "generate");
    let mut options = SpeechOptions::new("Joe: Hi\nJane: Hello");
    options.output_format = Some("pcm".to_owned());
    options.instructions = Some("Warmly".to_owned());
    options.provider_options = google_options(json!({
        "multiSpeakerVoiceConfig": {"speakerVoiceConfigs": [
            {"speaker": "Joe", "voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Kore"}}},
            {"speaker": "Jane", "voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Puck"}}}
        ]}
    }));
    let result = test
        .provider
        .speech("gemini-2.5-flash-preview-tts")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(
        features(&result.warnings),
        vec!["instructions", "outputFormat"]
    );
    assert_eq!(result.audio.len(), 8);
    assert_eq!(
        result
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("audio/L16;codec=pcm;rate=24000")
    );
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["contents"][0]["parts"][0]["text"],
        json!("Joe: Hi\nJane: Hello")
    );
    assert!(
        request["generationConfig"]["speechConfig"]["multiSpeakerVoiceConfig"]
            ["speakerVoiceConfigs"]
            .is_array()
    );
}

#[tokio::test]
async fn unknown_output_formats_fall_back_to_wav_with_a_warning() {
    let test = TestProvider::start().await;
    let mut options = SpeechOptions::new("Hello");
    options.output_format = Some("mp3".to_owned());
    options.voice = Some("Puck".to_owned());
    let prepared = test
        .provider
        .speech("gemini-2.5-flash-preview-tts")
        .prepare_request(&options)
        .unwrap();
    assert_eq!(features(&prepared.warnings), vec!["outputFormat"]);
    assert!(!prepared.raw_pcm);
    assert_eq!(
        prepared.body["generationConfig"]["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
        json!("Puck")
    );
}
