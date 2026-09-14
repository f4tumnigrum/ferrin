//! WebSocket-backed streaming transcription and speech translation against a
//! local mock server (`realtime` feature).

use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_spec::AudioFormat;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::SpeechTranslationModel;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamPart;
use futures_util::SinkExt;
use futures_util::StreamExt;
use http::header::SEC_WEBSOCKET_PROTOCOL;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::Request as HandshakeRequest;
use tokio_tungstenite::tungstenite::handshake::server::Response as HandshakeResponse;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::TestProvider;
use super::common::openai_options;

/// What the mock server observed.
#[derive(Default)]
struct Observed {
    path: Option<String>,
    protocols: Option<String>,
    has_authorization: bool,
    messages: Vec<JsonValue>,
}

/// A WebSocket server that answers a scripted event list once it sees the
/// `trigger` client event type.
struct MockWs {
    url: Url,
    observed: Arc<Mutex<Observed>>,
    _tasks: JoinSet<()>,
}

impl MockWs {
    async fn start(trigger: &'static str, script: Vec<JsonValue>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let observed = Arc::new(Mutex::new(Observed::default()));
        let sink = Arc::clone(&observed);
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let seen = Arc::clone(&sink);
            #[allow(
                clippy::result_large_err,
                reason = "the callback signature is fixed by tungstenite"
            )]
            let mut ws = tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &HandshakeRequest, mut response: HandshakeResponse| {
                    let mut seen = seen.lock().unwrap();
                    seen.path = Some(request.uri().to_string());
                    seen.has_authorization = request.headers().contains_key("authorization");
                    if let Some(protocols) = request.headers().get(SEC_WEBSOCKET_PROTOCOL) {
                        let all = protocols.to_str().unwrap().to_owned();
                        let first = all.split(',').next().unwrap().trim().to_owned();
                        seen.protocols = Some(all);
                        response
                            .headers_mut()
                            .insert(SEC_WEBSOCKET_PROTOCOL, first.parse().unwrap());
                    }
                    Ok(response)
                },
            )
            .await
            .unwrap();
            while let Some(Ok(message)) = ws.next().await {
                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) {
                        break;
                    }
                    continue;
                };
                let value: JsonValue = serde_json::from_str(text.as_str()).unwrap();
                let kind = value["type"].as_str().unwrap_or_default().to_owned();
                sink.lock().unwrap().messages.push(value);
                if kind == trigger {
                    for event in &script {
                        ws.send(Message::text(event.to_string())).await.unwrap();
                    }
                }
            }
        });
        Self {
            url: Url::parse(&format!("http://127.0.0.1:{port}/v1")).unwrap(),
            observed,
            _tasks: tasks,
        }
    }
}

fn audio() -> ferrin_spec::BoxStream<'static, Bytes> {
    futures_util::stream::iter(vec![
        Bytes::from_static(&[1, 2, 3]),
        Bytes::from_static(&[4, 5]),
    ])
    .boxed()
}

#[tokio::test]
async fn streaming_transcription_sends_session_audio_and_commit() {
    let server = MockWs::start(
        "input_audio_buffer.commit",
        vec![
            json!({"type": "transcription_session.updated"}),
            json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "Hello "}),
            json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "there"}),
            json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_1", "transcript": "Hello there"}),
        ],
    )
    .await;
    let base_url = server.url.clone();
    let test = TestProvider::start_with(move |mut settings| {
        settings.base_url = Some(base_url);
        settings
    })
    .await;
    let model = test.provider.transcription("gpt-realtime-whisper");
    assert!(model.supports_stream());
    let result = model
        .do_stream(TranscriptionStreamOptions {
            audio: audio(),
            input_audio_format: AudioFormat {
                kind: "audio/pcm".to_owned(),
                rate: Some(24_000),
            },
            provider_options: openai_options(json!({
                "language": "en",
                "prompt": "ignored",
                "streaming": {"delay": "low", "include": ["logprobs"]}
            })),
            headers: Headers::new(),
            include_raw_chunks: true,
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(
        result.request.body.as_ref().unwrap()["type"],
        json!("session.update")
    );
    let parts: Vec<TranscriptionStreamPart> = result.stream.collect().await;

    let TranscriptionStreamPart::StreamStart { warnings } = &parts[0] else {
        panic!("expected stream-start, got {:?}", parts[0]);
    };
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let deltas: String = parts
        .iter()
        .filter_map(|part| match part {
            TranscriptionStreamPart::TranscriptDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "Hello there");
    assert!(parts.iter().any(|part| matches!(
        part,
        TranscriptionStreamPart::TranscriptFinal { text, id: Some(id), .. }
            if text == "Hello there" && id == "item_1"
    )));
    let Some(TranscriptionStreamPart::Finish { text, language, .. }) = parts.last() else {
        panic!("expected finish, got {:?}", parts.last());
    };
    assert_eq!(text, "Hello there");
    assert_eq!(language.as_deref(), Some("en"));
    assert_eq!(
        parts
            .iter()
            .filter(|part| matches!(part, TranscriptionStreamPart::Raw { .. }))
            .count(),
        4
    );

    let observed = server.observed.lock().unwrap();
    assert_eq!(
        observed.path.as_deref(),
        Some("/v1/realtime?intent=transcription")
    );
    assert_eq!(
        observed.protocols.as_deref(),
        Some("realtime, openai-insecure-api-key.test-key")
    );
    assert!(!observed.has_authorization);
    let kinds: Vec<&str> = observed
        .messages
        .iter()
        .map(|message| message["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "session.update",
            "input_audio_buffer.append",
            "input_audio_buffer.append",
            "input_audio_buffer.commit"
        ]
    );
    let session = &observed.messages[0]["session"];
    assert_eq!(session["type"], json!("transcription"));
    assert_eq!(
        session["audio"]["input"]["format"],
        json!({"type": "audio/pcm", "rate": 24000})
    );
    assert_eq!(
        session["audio"]["input"]["transcription"],
        json!({"model": "gpt-realtime-whisper", "language": "en", "delay": "low"})
    );
    assert_eq!(session["audio"]["input"]["turn_detection"], json!(null));
    assert_eq!(session["include"], json!(["logprobs"]));
    assert_eq!(observed.messages[1]["audio"], json!("AQID"));
}

#[tokio::test]
async fn speech_translation_streams_audio_and_transcripts() {
    let server = MockWs::start(
        "session.close",
        vec![
            json!({"type": "session.input_transcript.delta", "delta": "Bonjour"}),
            json!({"type": "session.output_transcript.delta", "delta": "Hello"}),
            json!({"type": "session.output_audio.delta", "delta": "AQID"}),
            json!({"type": "session.closed"}),
        ],
    )
    .await;
    let base_url = server.url.clone();
    let test = TestProvider::start_with(move |mut settings| {
        settings.base_url = Some(base_url);
        settings
    })
    .await;
    let model = test.provider.speech_translation("gpt-realtime-translate");
    let result = model
        .do_stream(SpeechTranslationStreamOptions {
            audio: audio(),
            input_audio_format: AudioFormat {
                kind: "audio/pcm".to_owned(),
                rate: None,
            },
            target_language: "en".to_owned(),
            source_language: Some("fr".to_owned()),
            output_audio_format: None,
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            include_raw_chunks: false,
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    let parts: Vec<SpeechTranslationStreamPart> = result.stream.collect().await;
    let SpeechTranslationStreamPart::StreamStart { warnings } = &parts[0] else {
        panic!("expected stream-start");
    };
    assert_eq!(warnings.len(), 1);
    assert!(parts.iter().any(|part| matches!(
        part,
        SpeechTranslationStreamPart::Audio { audio, .. } if audio.as_ref() == [1, 2, 3]
    )));
    let Some(SpeechTranslationStreamPart::Finish {
        source_text,
        output_text,
        ..
    }) = parts.last()
    else {
        panic!("expected finish, got {:?}", parts.last());
    };
    assert_eq!(source_text, "Bonjour");
    assert_eq!(output_text, "Hello");
    let observed = server.observed.lock().unwrap();
    assert_eq!(
        observed.path.as_deref(),
        Some("/v1/realtime/translations?model=gpt-realtime-translate")
    );
    let kinds: Vec<&str> = observed
        .messages
        .iter()
        .map(|message| message["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "session.update",
            "session.input_audio_buffer.append",
            "session.input_audio_buffer.append",
            "session.close"
        ]
    );
    assert_eq!(
        observed.messages[0]["session"]["audio"]["output"]["language"],
        json!("en")
    );
}

#[tokio::test]
async fn speech_translation_validates_target_language_and_input_format() {
    let test = TestProvider::start().await;
    let model = test.provider.speech_translation("gpt-realtime-translate");
    let options = |target: &str, rate: Option<u32>| SpeechTranslationStreamOptions {
        audio: audio(),
        input_audio_format: AudioFormat {
            kind: "audio/pcm".to_owned(),
            rate,
        },
        target_language: target.to_owned(),
        source_language: None,
        output_audio_format: None,
        provider_options: ProviderOptions::new(),
        headers: Headers::new(),
        include_raw_chunks: false,
        cancellation: CancellationToken::new(),
    };
    let error = model.do_stream(options("", None)).await.unwrap_err();
    assert!(
        matches!(error, ferrin_spec::error::ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    let error = model
        .do_stream(options("en", Some(16_000)))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ferrin_spec::error::ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
}
