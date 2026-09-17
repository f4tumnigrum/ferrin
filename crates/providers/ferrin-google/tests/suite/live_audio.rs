//! Deterministic local WebSocket regressions; these are not live provider recordings.

mod boundaries;
mod lifecycle;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use base64::Engine;
use bytes::Bytes;
use ferrin_google::GoogleProvider;
use ferrin_google::GoogleSettings;
use ferrin_google::GoogleSpeechTranslationModel;
use ferrin_google::create_google;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::AudioFormat;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::dynamic::BoxStream;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_translation_model::SpeechTranslationModel;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart as Translation;
use ferrin_spec::speech_translation_model::SpeechTranslationUsage;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamPart as Transcript;
use futures_util::FutureExt;
use futures_util::SinkExt;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::Callback;
use tokio_tungstenite::tungstenite::handshake::server::ErrorResponse;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::handshake::server::Response;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::google_options;

struct Server {
    listener: TcpListener,
    provider: GoogleProvider,
}

struct CheckHandshake;

impl Callback for CheckHandshake {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert_eq!(
            request.uri().path(),
            "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
        );
        assert_eq!(request.uri().query(), Some("key=test-key"));
        assert!(!request.headers().contains_key("x-goog-api-key"));
        Ok(response)
    }
}

impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url =
            Url::parse(&format!("http://{}/v1beta", listener.local_addr().unwrap())).unwrap();
        let provider = create_google(GoogleSettings {
            base_url: Some(base_url.clone()),
            api_key: Some(SecretString::from("test-key")),
            name: Some("custom-google".to_owned()),
            url_policy: UrlPolicy::new().allow_http().trust_origin(&base_url),
            ..GoogleSettings::default()
        })
        .unwrap();
        Self { listener, provider }
    }

    async fn accept(&self) -> WebSocketStream<TcpStream> {
        let (stream, _) = self.listener.accept().await.unwrap();
        accept_hdr_async(stream, CheckHandshake).await.unwrap()
    }

    fn translation(&self) -> GoogleSpeechTranslationModel {
        GoogleSpeechTranslationModel::new(
            self.provider.config().clone(),
            "gemini-3.5-live-translate-preview",
        )
    }
}

async fn read(socket: &mut WebSocketStream<TcpStream>) -> JsonValue {
    let message = socket.next().await.unwrap().unwrap();
    serde_json::from_slice(&message.into_data()).unwrap()
}

async fn send(socket: &mut WebSocketStream<TcpStream>, value: JsonValue) {
    socket.send(Message::text(value.to_string())).await.unwrap();
}

fn transcription(audio: BoxStream<'static, Bytes>) -> TranscriptionStreamOptions {
    TranscriptionStreamOptions {
        audio,
        input_audio_format: AudioFormat::with_rate("audio/pcm", 16_000),
        provider_options: ProviderOptions::new(),
        headers: Headers::new(),
        include_raw_chunks: false,
        cancellation: CancellationToken::new(),
    }
}

fn translation(audio: BoxStream<'static, Bytes>) -> SpeechTranslationStreamOptions {
    SpeechTranslationStreamOptions {
        audio,
        input_audio_format: AudioFormat::new("pcm16"),
        target_language: "es".to_owned(),
        source_language: None,
        output_audio_format: None,
        provider_options: ProviderOptions::new(),
        headers: Headers::new(),
        include_raw_chunks: false,
        cancellation: CancellationToken::new(),
    }
}

fn audio() -> BoxStream<'static, Bytes> {
    Box::pin(futures_util::stream::iter([Bytes::from_static(&[
        1, 0, 2, 0,
    ])]))
}

async fn acknowledge_and_drain(socket: &mut WebSocketStream<TcpStream>) {
    send(socket, json!({"setupComplete": {}})).await;
    loop {
        let value = read(socket).await;
        if value["realtimeInput"]["audioStreamEnd"] == true {
            break;
        }
        assert_eq!(
            value["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
    }
}

#[tokio::test]
async fn transcription_gates_audio_and_maps_revisions_segments_and_custom_metadata() {
    let server = Server::new().await;
    let polls = Arc::new(AtomicUsize::new(0));
    let count = polls.clone();
    let mut sent = false;
    let audio = Box::pin(futures_util::stream::poll_fn(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Ready(if sent {
            None
        } else {
            sent = true;
            Some(Bytes::from_static(&[1, 0]))
        })
    }));
    let mut options = transcription(audio);
    options.provider_options =
        google_options(json!({"languageCodes":["en"], "mode":"SMART", "wordTimestamp":true}));
    let serve = async {
        let mut socket = server.accept().await;
        assert_eq!(
            read(&mut socket).await,
            json!({"setup": {
                "model":"models/gemini-3.5-transcribe-live", "inputAudioTranscription": {"languageCodes":["en"],"mode":"SMART","wordTimestamp":true},
            }})
        );
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        acknowledge_and_drain(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent":{"interimInputTranscription":{"text":"help"}}}),
        )
        .await;
        send(&mut socket, json!({"serverContent":{"inputTranscription":{"text":"Hello", "languageCode":"en", "finished":true}}})).await;
        send(
            &mut socket,
            json!({"inputTranscription":{"text":"world", "finished":true}}),
        )
        .await;
        send(&mut socket, json!({"usageMetadata":{"totalTokenCount":12},"serverContent":{"interactionStatus":"IDLE"}})).await;
    };
    let collect = async {
        let model = server.provider.transcription("gemini-3.5-transcribe-live");
        assert!(model.supports_stream());
        model
            .do_stream(options)
            .await
            .unwrap()
            .stream
            .collect::<Vec<_>>()
            .await
    };
    let ((), parts) = tokio::join!(serve, collect);
    let expected_metadata = ferrin_spec::ProviderMetadata::from_iter([
        (
            "google".to_owned(),
            serde_json::from_value(json!({"usageMetadata":{"totalTokenCount":12}})).unwrap(),
        ),
        (
            "custom-google".to_owned(),
            serde_json::from_value(json!({"usageMetadata":{"totalTokenCount":12}})).unwrap(),
        ),
    ]);
    assert_eq!(
        parts,
        vec![
            Transcript::StreamStart { warnings: vec![] },
            Transcript::TranscriptPartial {
                id: Some("google-segment-0".to_owned()),
                text: "help".to_owned(),
                start_second: None,
                duration_in_seconds: None,
                channel_index: None,
                provider_metadata: None
            },
            Transcript::TranscriptDelta {
                id: Some("google-segment-0".to_owned()),
                delta: "Hello".to_owned(),
                provider_metadata: None
            },
            Transcript::TranscriptFinal {
                id: Some("google-segment-0".to_owned()),
                text: "Hello".to_owned(),
                start_second: None,
                end_second: None,
                channel_index: None,
                provider_metadata: None
            },
            Transcript::TranscriptDelta {
                id: Some("google-segment-1".to_owned()),
                delta: "world".to_owned(),
                provider_metadata: None
            },
            Transcript::TranscriptFinal {
                id: Some("google-segment-1".to_owned()),
                text: "world".to_owned(),
                start_second: None,
                end_second: None,
                channel_index: None,
                provider_metadata: None
            },
            Transcript::Finish {
                text: "Hello world".to_owned(),
                segments: vec![],
                language: Some("en".to_owned()),
                duration_in_seconds: None,
                provider_metadata: Some(expected_metadata)
            },
        ]
    );
}

#[tokio::test]
async fn translation_maps_audio_text_usage_and_continuous_silence_completion() {
    let server = Server::new().await;
    let mut options = translation(audio());
    options.provider_options = google_options(json!({"echoTargetLanguage":true}));
    options.include_raw_chunks = true;
    let serve = async {
        let mut socket = server.accept().await;
        assert_eq!(
            read(&mut socket).await,
            json!({"setup": {
                "model":"models/gemini-3.5-live-translate-preview", "generationConfig": {
                    "responseModalities":["AUDIO"], "translationConfig":{"targetLanguageCode":"es", "echoTargetLanguage":true},
                }, "inputAudioTranscription":{},"outputAudioTranscription":{},
            }})
        );
        acknowledge_and_drain(&mut socket).await;
        send(&mut socket,json!({"serverContent":{"inputTranscription":{"text":"Hello"},"outputTranscription":{"text":"Hola"}},"usageMetadata":{"promptTokensDetails":[{"modality":"AUDIO","tokenCount":7},{"modality":"TEXT","tokenCount":999}],"responseTokensDetails":[{"modality":"AUDIO","tokenCount":9}]}})).await;
        send(&mut socket,json!({"usageMetadata":{"promptTokensDetails":[{"modality":"AUDIO","tokenCount":3}],"responseTokensDetails":[{"modality":"AUDIO","tokenCount":2}]}})).await;
        send(&mut socket,json!({"serverContent":{"modelTurn":{"parts":[{"inlineData":{"data":base64::engine::general_purpose::STANDARD.encode(vec![0;48_000])}}]}}})).await;
    };
    let collect = async {
        server
            .translation()
            .do_stream(options)
            .await
            .unwrap()
            .stream
            .collect::<Vec<_>>()
            .await
    };
    let ((), parts) = tokio::join!(serve, collect);
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, Translation::Raw { .. }))
    );
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, Translation::Audio { audio, .. } if audio.len()==48_000))
    );
    let finish = parts.into_iter().last().unwrap();
    let usage = json!({"usageMetadata": {
        "promptTokensDetails":[{"modality":"AUDIO","tokenCount":3}],
        "responseTokensDetails":[{"modality":"AUDIO","tokenCount":2}],
    }});
    let provider_metadata = ferrin_spec::ProviderMetadata::from_iter([
        (
            "google".to_owned(),
            serde_json::from_value(usage.clone()).unwrap(),
        ),
        (
            "custom-google".to_owned(),
            serde_json::from_value(usage).unwrap(),
        ),
    ]);
    assert_eq!(
        finish,
        Translation::Finish {
            source_text: "Hello".to_owned(),
            output_text: "Hola".to_owned(),
            duration_in_seconds: None,
            usage: Some(SpeechTranslationUsage {
                input_audio_tokens: Some(10),
                output_audio_tokens: Some(11),
                ..SpeechTranslationUsage::default()
            }),
            provider_metadata: Some(provider_metadata),
        }
    );
}

#[tokio::test]
async fn premature_eof_and_malformed_json_are_terminal_errors() {
    for malformed in [false, true] {
        let server = Server::new().await;
        let serve = async {
            let mut socket = server.accept().await;
            read(&mut socket).await;
            if malformed {
                socket.send(Message::text("{invalid")).await.unwrap();
            } else {
                socket.close(None).await.unwrap();
            }
        };
        let collect = async {
            server
                .provider
                .transcription("test-live")
                .do_stream(transcription(audio()))
                .await
                .unwrap()
                .stream
                .collect::<Vec<_>>()
                .await
        };
        let ((), parts) = tokio::join!(serve, collect);
        assert!(matches!(
            parts.as_slice(),
            [Transcript::StreamStart { .. }, Transcript::Error { .. }]
        ));
    }
}

#[tokio::test]
async fn cancellation_and_drop_release_input_without_detached_tasks() {
    let server = Server::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    struct DropInput(Arc<AtomicUsize>);
    impl Drop for DropInput {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let guard = DropInput(dropped.clone());
    let input = futures_util::stream::poll_fn(move |_| {
        let _keep_alive = &guard;
        std::task::Poll::<Option<Bytes>>::Pending
    });
    let options = transcription(Box::pin(input));
    let cancellation = options.cancellation.clone();
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        socket.next().await
    };
    let collect = async {
        let mut stream = server
            .provider
            .transcription("test-live")
            .do_stream(options)
            .await
            .unwrap()
            .stream;
        assert!(matches!(
            stream.next().await,
            Some(Transcript::StreamStart { .. })
        ));
        cancellation.cancel();
        assert!(matches!(
            stream.next().await,
            Some(Transcript::Error { .. })
        ));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(stream.next().await.is_none());
    };
    let _ = tokio::join!(serve, collect);
    assert!(cancellation.is_cancelled());
}

#[tokio::test]
async fn invalid_input_and_insecure_destinations_fail_before_connection() {
    let server = Server::new().await;
    let mut options = translation(audio());
    options.target_language.clear();
    assert!(matches!(
        server.translation().do_stream(options).await,
        Err(ProviderError::InvalidArgument(_))
    ));
    let mut options = transcription(audio());
    options.input_audio_format = AudioFormat::new("mp3");
    assert!(matches!(
        server
            .provider
            .transcription("test-live")
            .do_stream(options)
            .await,
        Err(ProviderError::InvalidArgument(_))
    ));
    assert!(server.listener.accept().now_or_never().is_none());
    let provider = create_google(GoogleSettings {
        base_url: Some(Url::parse("https://127.0.0.1/v1beta").unwrap()),
        api_key: Some(SecretString::from("secret-never-in-errors")),
        ..GoogleSettings::default()
    })
    .unwrap();
    let error = provider
        .transcription("test-live")
        .do_stream(transcription(audio()))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::InvalidArgument(_)));
    assert!(!error.to_string().contains("secret-never-in-errors"));
}
