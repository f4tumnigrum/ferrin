//! Live audio completion and bounded-response edge cases.

use std::time::Duration;

use ferrin_google::GoogleProvider;
use ferrin_google::GoogleSettings;
use ferrin_google::create_google;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionStreamPart as Transcript;
use futures_util::SinkExt;
use futures_util::StreamExt;
use secrecy::SecretString;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::Server;
use super::acknowledge_and_drain;
use super::audio;
use super::read;
use super::send;
use super::transcription;

#[tokio::test]
async fn normal_close_after_audio_end_finishes_but_abnormal_close_is_terminal_error() {
    for code in [CloseCode::Normal, CloseCode::Error] {
        let server = Server::new().await;
        let serve = async {
            let mut socket = server.accept().await;
            read(&mut socket).await;
            acknowledge_and_drain(&mut socket).await;
            send(&mut socket, json!({"inputTranscription":{"text":"hello"}})).await;
            socket
                .close(Some(CloseFrame {
                    code,
                    reason: "fixture".into(),
                }))
                .await
                .unwrap();
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
        if code == CloseCode::Normal {
            assert!(matches!(parts.last(),Some(Transcript::Finish{text,..}) if text=="hello"));
        } else {
            assert!(matches!(parts.last(), Some(Transcript::Error { .. })));
            assert!(
                !parts
                    .iter()
                    .any(|part| matches!(part, Transcript::Finish { .. }))
            );
        }
    }
}

fn limited_provider(server: &Server) -> GoogleProvider {
    let config = server.provider.config();
    create_google(GoogleSettings {
        base_url: Some(config.base_url.clone()),
        api_key: Some(SecretString::from("test-key")),
        url_policy: config.url_policy.clone().max_body_bytes(64),
        ..GoogleSettings::default()
    })
    .unwrap()
}

#[tokio::test]
async fn oversized_websocket_messages_fail_with_no_successful_finish() {
    let server = Server::new().await;
    let provider = limited_provider(&server);
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        send(
            &mut socket,
            json!({"inputTranscription":{"text":"x".repeat(128)}}),
        )
        .await;
    };
    let collect = async {
        provider
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

#[tokio::test]
async fn backpressure_does_not_discard_buffered_transcripts_after_quiet_deadline() {
    let server = Server::new().await;
    let (buffered, wait_buffered) = tokio::sync::oneshot::channel();
    let (done, wait_done) = tokio::sync::oneshot::channel();
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        acknowledge_and_drain(&mut socket).await;
        send(&mut socket, json!({"inputTranscription":{"text":"hello "}})).await;
        send(&mut socket,json!({"inputTranscription":{"text":"world"},"serverContent":{"interactionStatus":"IDLE"}})).await;
        socket.flush().await.unwrap();
        buffered.send(()).unwrap();
        wait_done.await.unwrap();
    };
    let collect = async {
        let mut stream = server
            .provider
            .transcription("test-live")
            .do_stream(transcription(audio()))
            .await
            .unwrap()
            .stream;
        assert!(matches!(
            stream.next().await,
            Some(Transcript::StreamStart { .. })
        ));
        assert!(matches!(
            stream.next().await,
            Some(Transcript::TranscriptDelta { .. })
        ));
        wait_buffered.await.unwrap();
        // This models a consumer pausing after the first transcript chunk.
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(2)).await;
        let parts = stream.collect::<Vec<_>>().await;
        assert!(matches!(parts.last(),Some(Transcript::Finish{text,..}) if text=="hello world"));
        done.send(()).unwrap();
    };
    tokio::join!(serve, collect);
}
