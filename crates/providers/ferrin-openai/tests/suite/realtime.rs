//! Realtime model: client secrets, session config and event mapping.

use ferrin_spec::AudioFormat;
use ferrin_spec::RealtimeFactory;
use ferrin_spec::RealtimeModel;
use ferrin_spec::realtime_model::ClientSecretOptions;
use ferrin_spec::realtime_model::ConversationItem;
use ferrin_spec::realtime_model::ConversationRole;
use ferrin_spec::realtime_model::GetTokenOptions;
use ferrin_spec::realtime_model::Modality;
use ferrin_spec::realtime_model::RealtimeClientEvent;
use ferrin_spec::realtime_model::RealtimeServerEvent;
use ferrin_spec::realtime_model::RealtimeSessionConfig;
use ferrin_spec::realtime_model::RealtimeToolDefinition;
use ferrin_spec::realtime_model::ResponseCreateOptions;
use ferrin_spec::realtime_model::TranscriptionConfig;
use ferrin_spec::realtime_model::TurnDetection;
use ferrin_spec::realtime_model::TurnDetectionKind;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::TestProvider;

fn session_config() -> RealtimeSessionConfig {
    RealtimeSessionConfig {
        instructions: Some("Be helpful.".to_owned()),
        voice: Some("marin".to_owned()),
        output_modalities: Some(vec![Modality::Audio]),
        input_audio_format: Some(AudioFormat {
            kind: "audio/pcm".to_owned(),
            rate: Some(24_000),
        }),
        input_audio_transcription: Some(TranscriptionConfig {
            model: None,
            language: Some("en".to_owned()),
            prompt: None,
        }),
        output_audio_transcription: None,
        output_audio_format: Some(AudioFormat {
            kind: "audio/pcm".to_owned(),
            rate: None,
        }),
        turn_detection: Some(TurnDetection {
            kind: TurnDetectionKind::SemanticVad,
            threshold: Some(0.6),
            silence_duration_ms: Some(400),
            prefix_padding_ms: None,
        }),
        tools: vec![RealtimeToolDefinition {
            name: "get_weather".to_owned(),
            description: Some("Weather".to_owned()),
            parameters: json!({"type": "object", "properties": {}}),
        }],
        provider_options: Some(serde_json::from_value(json!({"max_output_tokens": 200})).unwrap()),
    }
}

#[tokio::test]
async fn client_secret_posts_session_and_returns_wss_url() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/realtime/client_secrets",
        "realtime",
        "client-secret",
    );
    let model = test.provider.realtime().realtime_model("gpt-realtime");
    assert_eq!(model.provider().as_str(), "openai.realtime");
    let secret = model
        .do_create_client_secret(ClientSecretOptions {
            expires_after_seconds: Some(600),
            session_config: Some(session_config()),
        })
        .await
        .unwrap();
    assert_eq!(secret.token, "ek_test_secret");
    assert_eq!(secret.expires_at, Some(1_757_726_400));
    assert_eq!(secret.url.scheme(), "ws");
    assert_eq!(secret.url.path(), "/v1/realtime");
    assert_eq!(secret.url.query(), Some("model=gpt-realtime"));
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["expires_after"],
        json!({"anchor": "created_at", "seconds": 600})
    );
    assert_eq!(request["session"]["type"], json!("realtime"));
    assert_eq!(request["session"]["model"], json!("gpt-realtime"));
    insta::assert_json_snapshot!("realtime_session_config", request["session"]);
}

#[tokio::test]
async fn factory_get_token_uses_the_requested_model() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1/realtime/client_secrets",
        "realtime",
        "client-secret",
    );
    let secret = test
        .provider
        .realtime()
        .get_token(GetTokenOptions {
            model: "gpt-realtime-mini".into(),
            expires_after_seconds: None,
            session_config: None,
        })
        .await
        .unwrap();
    assert_eq!(secret.url.query(), Some("model=gpt-realtime-mini"));
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({"session": {"type": "realtime", "model": "gpt-realtime-mini"}})
    );
}

#[tokio::test]
async fn websocket_config_carries_the_token_as_sub_protocol() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model("gpt-realtime");
    let url = Url::parse("wss://api.openai.com/v1/realtime?model=gpt-realtime").unwrap();
    let config = model.websocket_config("ek_abc", &url);
    assert_eq!(config.url, url);
    assert_eq!(
        config.protocols,
        vec![
            "realtime".to_owned(),
            "openai-insecure-api-key.ek_abc".to_owned()
        ]
    );
}

#[tokio::test]
async fn client_events_serialize_to_the_wire_format() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model("gpt-realtime");
    let events = vec![
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
                call_id: "call_1".to_owned(),
                name: Some("get_weather".to_owned()),
                output: "{\"temp\":21}".to_owned(),
            },
        },
        RealtimeClientEvent::ConversationItemTruncate {
            item_id: "item_1".to_owned(),
            content_index: 0,
            audio_end_ms: 1500,
        },
        RealtimeClientEvent::ResponseCreate {
            options: Some(ResponseCreateOptions {
                modalities: Some(vec!["text".to_owned()]),
                instructions: Some("Short".to_owned()),
                metadata: None,
            }),
        },
        RealtimeClientEvent::ResponseCancel,
    ];
    let mut serialized = Vec::new();
    for event in events {
        serialized.push(model.serialize_client_event(event).await.unwrap());
    }
    insta::assert_json_snapshot!("realtime_client_events", serialized);
}

#[tokio::test]
async fn server_events_map_to_standard_events() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model("gpt-realtime");
    let parse = |raw: serde_json::Value| model.parse_server_event(raw).unwrap().remove(0);
    assert!(matches!(
        parse(json!({"type": "session.created", "session": {"id": "sess_1"}})),
        RealtimeServerEvent::SessionCreated { session_id: Some(id), .. } if id == "sess_1"
    ));
    assert!(matches!(
        parse(json!({"type": "response.done", "response": {"id": "resp_1", "status": "completed"}})),
        RealtimeServerEvent::ResponseDone { response_id, status, .. }
            if response_id == "resp_1" && status == "completed"
    ));
    let RealtimeServerEvent::AudioDelta { delta, item_id, .. } = parse(json!({
        "type": "response.output_audio.delta",
        "response_id": "resp_1",
        "item_id": "item_1",
        "delta": "AQI="
    })) else {
        panic!("expected audio delta");
    };
    assert_eq!(delta.as_ref(), b"\x01\x02");
    assert_eq!(item_id, "item_1");
    assert!(matches!(
        parse(json!({
            "type": "response.function_call_arguments.done",
            "response_id": "resp_1",
            "item_id": "item_2",
            "call_id": "call_1",
            "name": "get_weather",
            "arguments": "{}"
        })),
        RealtimeServerEvent::FunctionCallArgumentsDone { call_id, name, arguments, .. }
            if call_id == "call_1" && name == "get_weather" && arguments == "{}"
    ));
    assert!(matches!(
        parse(json!({"type": "error", "error": {"message": "boom", "code": "bad"}})),
        RealtimeServerEvent::Error { message, code: Some(code), .. }
            if message == "boom" && code == "bad"
    ));
    assert!(matches!(
        parse(json!({"type": "rate_limits.updated"})),
        RealtimeServerEvent::Custom { raw_type, .. } if raw_type == "rate_limits.updated"
    ));
    let error = model
        .parse_server_event(json!({
            "type": "response.output_audio.delta",
            "response_id": "r",
            "item_id": "i",
            "delta": "not base64!"
        }))
        .unwrap_err();
    assert!(
        matches!(
            error,
            ferrin_spec::error::ProviderError::InvalidResponseData(_)
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn disabled_turn_detection_serializes_as_null() {
    let test = TestProvider::start().await;
    let model = test.provider.realtime().realtime_model("gpt-realtime");
    let config = RealtimeSessionConfig {
        turn_detection: Some(TurnDetection {
            kind: TurnDetectionKind::Disabled,
            threshold: None,
            silence_duration_ms: None,
            prefix_padding_ms: None,
        }),
        ..RealtimeSessionConfig::default()
    };
    let session = model.build_session_config(&config).unwrap();
    assert_eq!(session["audio"]["input"]["turn_detection"], json!(null));
    assert!(session.get("tools").is_none());
}
