//! Live API realtime model: auth tokens, session setup and event mapping.

use ferrin_spec::AudioFormat;
use ferrin_spec::RealtimeFactory;
use ferrin_spec::RealtimeModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::realtime_model::ClientSecretOptions;
use ferrin_spec::realtime_model::ConversationItem;
use ferrin_spec::realtime_model::ConversationRole;
use ferrin_spec::realtime_model::GetTokenOptions;
use ferrin_spec::realtime_model::Modality;
use ferrin_spec::realtime_model::RealtimeClientEvent;
use ferrin_spec::realtime_model::RealtimeServerEvent;
use ferrin_spec::realtime_model::RealtimeSessionConfig;
use ferrin_spec::realtime_model::RealtimeToolDefinition;
use ferrin_spec::realtime_model::TranscriptionConfig;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::TestProvider;

const MODEL: &str = "gemini-2.5-flash-native-audio-preview";
const WS_PATH: &str =
    "/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained";

fn session_config() -> RealtimeSessionConfig {
    RealtimeSessionConfig {
        instructions: Some("Be helpful.".to_owned()),
        voice: Some("Kore".to_owned()),
        output_modalities: Some(vec![Modality::Audio]),
        input_audio_format: Some(AudioFormat {
            kind: "audio/pcm".to_owned(),
            rate: Some(24_000),
        }),
        input_audio_transcription: Some(TranscriptionConfig {
            model: None,
            language: None,
            prompt: None,
        }),
        output_audio_transcription: None,
        output_audio_format: None,
        turn_detection: None,
        tools: vec![RealtimeToolDefinition {
            name: "get_weather".to_owned(),
            description: Some("Weather".to_owned()),
            parameters: json!({
                "type": "object",
                "properties": {"city": {"type": ["string", "null"]}},
                "required": ["city"]
            }),
        }],
        provider_options: Some(
            serde_json::from_value(json!({
                "google": {"translationConfig": {"targetLanguageCode": "de"}},
                "sessionResumption": {}
            }))
            .unwrap(),
        ),
    }
}

#[tokio::test]
async fn client_secret_posts_an_auth_token_request_with_the_key_in_the_query() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1alpha/auth_tokens",
        "realtime",
        "auth-token",
    );
    let model = test.provider.realtime().realtime_model(MODEL);
    assert_eq!(model.provider().as_str(), "google.realtime");
    let secret = model
        .do_create_client_secret(ClientSecretOptions {
            expires_after_seconds: Some(600),
            session_config: Some(session_config()),
        })
        .await
        .unwrap();
    assert_eq!(secret.token, "auth_tokens/token-abc");
    assert_eq!(secret.expires_at, Some(1_789_376_400));
    assert_eq!(secret.url.scheme(), "ws");
    assert_eq!(secret.url.path(), WS_PATH);
    let request = test.only_request();
    assert_eq!(request.query.as_deref(), Some("key=test-key"));
    assert_eq!(request.header("x-goog-api-key"), None);
    let body = request.body_json().unwrap();
    assert_eq!(body["uses"], json!(0));
    assert!(body["expireTime"].is_string());
    assert!(body["newSessionExpireTime"].is_string());
    insta::assert_json_snapshot!("realtime_setup", body["bidiGenerateContentSetup"]);
}

#[tokio::test]
async fn factory_get_token_uses_the_requested_model() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1alpha/auth_tokens",
        "realtime",
        "auth-token",
    );
    let secret = test
        .provider
        .realtime()
        .get_token(GetTokenOptions {
            model: "gemini-live-2.5-flash".into(),
            expires_after_seconds: None,
            session_config: None,
        })
        .await
        .unwrap();
    assert_eq!(secret.token, "auth_tokens/token-abc");
    let body = test.only_request().body_json().unwrap();
    assert_eq!(
        body["bidiGenerateContentSetup"],
        json!({
            "model": "models/gemini-live-2.5-flash",
            "generationConfig": {"responseModalities": ["AUDIO"]}
        })
    );
}

#[tokio::test]
async fn websocket_config_carries_the_token_as_a_query_parameter() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model(MODEL);
    let url = Url::parse(&format!("wss://example.test{WS_PATH}")).unwrap();
    let config = model.websocket_config("auth_tokens/abc", &url);
    assert_eq!(config.url.query(), Some("access_token=auth_tokens%2Fabc"));
    assert!(config.protocols.is_empty());
}

#[tokio::test]
async fn client_events_serialize_to_the_wire_format() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model(MODEL);
    let events = vec![
        RealtimeClientEvent::SessionUpdate {
            config: Box::new(session_config()),
        },
        RealtimeClientEvent::InputAudioAppend {
            audio: bytes::Bytes::from_static(b"\x01\x02"),
        },
        RealtimeClientEvent::InputAudioCommit,
        RealtimeClientEvent::ConversationItemCreate {
            item: ConversationItem::TextMessage {
                role: ConversationRole::User,
                text: "Hello".to_owned(),
            },
        },
        RealtimeClientEvent::ConversationItemCreate {
            item: ConversationItem::FunctionCallOutput {
                call_id: "call-1".to_owned(),
                name: Some("get_weather".to_owned()),
                output: "{\"temp\":21}".to_owned(),
            },
        },
    ];
    let mut serialized = Vec::new();
    for event in events {
        serialized.push(model.serialize_client_event(event).await.unwrap());
    }
    assert_eq!(
        serialized[1]["realtimeInput"]["audio"]["mimeType"],
        json!("audio/pcm;rate=24000")
    );
    insta::assert_json_snapshot!("realtime_client_events", serialized);
    // Events without a Live API equivalent are rejected instead of being sent
    // as `null`.
    for event in [
        RealtimeClientEvent::InputAudioClear,
        RealtimeClientEvent::ResponseCancel,
    ] {
        assert!(matches!(
            model.serialize_client_event(event).await,
            Err(ProviderError::UnsupportedFunctionality(_))
        ));
    }
}

#[tokio::test]
async fn server_events_map_to_standard_events_with_turn_ids() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model(MODEL);
    let parse = |raw: serde_json::Value| model.parse_server_event(raw).unwrap();
    assert!(matches!(
        parse(json!({"setupComplete": {}}))[0],
        RealtimeServerEvent::SessionCreated { .. }
    ));
    let events = parse(json!({"serverContent": {
        "modelTurn": {"parts": [{"inlineData": {"mimeType": "audio/pcm;rate=24000", "data": "AQI="}}]},
        "outputTranscription": {"text": "Hel"}
    }}));
    assert!(matches!(
        &events[0],
        RealtimeServerEvent::AudioDelta { response_id, item_id, delta, .. }
            if response_id == "google-resp-0" && item_id == "google-item-0" && delta.as_ref() == b"\x01\x02"
    ));
    assert!(matches!(
        &events[1],
        RealtimeServerEvent::AudioTranscriptDelta { delta, .. } if delta == "Hel"
    ));
    let events =
        parse(json!({"serverContent": {"generationComplete": true, "turnComplete": true}}));
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| match event {
            RealtimeServerEvent::Custom { raw_type, .. } => raw_type.as_str(),
            RealtimeServerEvent::AudioDone { .. } => "audio-done",
            RealtimeServerEvent::AudioTranscriptDone { .. } => "transcript-done",
            RealtimeServerEvent::ResponseDone { status, .. } => status.as_str(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "generationComplete",
            "audio-done",
            "transcript-done",
            "completed"
        ]
    );
    let events = parse(json!({"serverContent": {"modelTurn": {"parts": [{"text": "Next"}]}}}));
    assert!(matches!(
        &events[0],
        RealtimeServerEvent::TextDelta { response_id, delta, .. }
            if response_id == "google-resp-1" && delta == "Next"
    ));
    let events = parse(json!({"toolCall": {"functionCalls": [
        {"id": "fc-1", "name": "get_weather", "args": {"city": "Berlin"}}
    ]}}));
    assert!(matches!(
        &events[1],
        RealtimeServerEvent::FunctionCallArgumentsDone { call_id, name, arguments, .. }
            if call_id == "fc-1" && name == "get_weather" && arguments == "{\"city\":\"Berlin\"}"
    ));
    assert!(matches!(
        &parse(json!({"serverContent": {"interrupted": true}}))[0],
        RealtimeServerEvent::SpeechStarted { .. }
    ));
    assert!(matches!(
        &parse(json!({"inputTranscription": {"text": "hi there"}}))[0],
        RealtimeServerEvent::InputTranscriptionCompleted { transcript, .. } if transcript == "hi there"
    ));
    assert!(matches!(
        &parse(json!({"goAway": {"timeLeft": "10s"}}))[0],
        RealtimeServerEvent::Custom { raw_type, .. } if raw_type == "goAway"
    ));
    assert!(matches!(
        &parse(json!({"somethingNew": 1}))[0],
        RealtimeServerEvent::Custom { raw_type, .. } if raw_type == "somethingNew"
    ));
}

#[tokio::test]
async fn function_outputs_preserve_every_json_type_and_plain_text() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model(MODEL);
    for (output, response) in [
        ("Sunny", json!({"result": "Sunny"})),
        ("42", json!({"result": 42})),
        ("[1,2]", json!({"result": [1,2]})),
        ("null", json!({"result": null})),
        ("false", json!({"result": false})),
        ("\"Sunny\"", json!({"result": "Sunny"})),
        ("{\"temp\":21}", json!({"temp": 21})),
        ("", json!({"result": ""})),
    ] {
        let serialized = model
            .serialize_client_event(RealtimeClientEvent::ConversationItemCreate {
                item: ConversationItem::FunctionCallOutput {
                    call_id: "call-1".to_owned(),
                    name: Some("weather".to_owned()),
                    output: output.to_owned(),
                },
            })
            .await
            .unwrap();
        assert_eq!(
            serialized,
            json!({"toolResponse": {"functionResponses": [{
                "id":"call-1", "name":"weather", "response":response
            }]}})
        );
    }
}
