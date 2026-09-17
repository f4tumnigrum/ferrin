//! Audio stream lifetime, timer, backpressure and validation regressions.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use ferrin_spec::speech_translation_model::SpeechTranslationModel;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart as Translation;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionStreamPart as Transcript;
use futures_util::FutureExt;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::Server;
use super::acknowledge_and_drain;
use super::audio;
use super::read;
use super::send;
use super::transcription;
use super::translation;

#[tokio::test]
async fn quiet_grace_finalizes_latest_interim_without_turn_complete() {
    let server = Server::new().await;
    let (done, wait_done) = tokio::sync::oneshot::channel();
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        acknowledge_and_drain(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent":{"interimInputTranscription":{"text":"revised text"}}}),
        )
        .await;
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
            Some(Transcript::TranscriptPartial { .. })
        ));
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(500)).await;
        assert!(stream.next().now_or_never().is_none());
        tokio::time::advance(Duration::from_millis(501)).await;
        let parts = stream.collect::<Vec<_>>().await;
        assert_eq!(
            parts,
            vec![
                Transcript::TranscriptFinal {
                    id: Some("google-segment-0".to_owned()),
                    text: "revised text".to_owned(),
                    start_second: None,
                    end_second: None,
                    channel_index: None,
                    provider_metadata: None
                },
                Transcript::Finish {
                    text: "revised text".to_owned(),
                    segments: vec![],
                    language: None,
                    duration_in_seconds: None,
                    provider_metadata: None
                },
            ]
        );
        done.send(()).unwrap();
    };
    tokio::join!(serve, collect);
}

#[tokio::test]
async fn stream_drop_releases_unread_input_and_does_not_cancel_caller() {
    let server = Server::new().await;
    let drops = Arc::new(AtomicUsize::new(0));
    struct DropGuard(Arc<AtomicUsize>);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let guard = DropGuard(drops.clone());
    let input = futures_util::stream::poll_fn(move |_| {
        let _keep = &guard;
        std::task::Poll::<Option<Bytes>>::Pending
    });
    let options = transcription(Box::pin(input));
    let cancellation = options.cancellation.clone();
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        socket.next().await
    };
    let client = async {
        let stream = server
            .provider
            .transcription("test-live")
            .do_stream(options)
            .await
            .unwrap()
            .stream;
        drop(stream);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(!cancellation.is_cancelled());
    };
    let _ = tokio::join!(serve, client);
}

#[tokio::test]
async fn output_backpressure_stops_polling_input_until_next_part_requested() {
    let server = Server::new().await;
    let polls = Arc::new(AtomicUsize::new(0));
    let count = polls.clone();
    let input = futures_util::stream::poll_fn(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Ready(Some(Bytes::from_static(&[1, 0])))
    });
    let (seen, wait_seen) = tokio::sync::oneshot::channel();
    let (release, wait_release) = tokio::sync::oneshot::channel();
    let serve = async {
        let mut socket = server.accept().await;
        read(&mut socket).await;
        send(&mut socket, json!({"setupComplete":{}})).await;
        seen.send(()).unwrap();
        wait_release.await.unwrap();
    };
    let client = async {
        let mut stream = server
            .provider
            .transcription("test-live")
            .do_stream(transcription(Box::pin(input)))
            .await
            .unwrap()
            .stream;
        assert!(matches!(
            stream.next().await,
            Some(Transcript::StreamStart { .. })
        ));
        wait_seen.await.unwrap();
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        release.send(()).unwrap();
        // Holding the stream without polling never starts an input producer.
        tokio::task::yield_now().await;
        drop(stream);
    };
    tokio::join!(serve, client);
}

#[tokio::test]
async fn translation_invalid_audio_and_truncated_connections_fail_without_finish() {
    for data in [Some("invalid-base64!"), None] {
        let server = Server::new().await;
        let serve = async {
            let mut socket = server.accept().await;
            read(&mut socket).await;
            acknowledge_and_drain(&mut socket).await;
            if let Some(data) = data {
                send(
                    &mut socket,
                    json!({"serverContent":{"modelTurn":{"parts":[{"inlineData":{"data":data}}]}}}),
                )
                .await;
            }
        };
        let client = async {
            server
                .translation()
                .do_stream(translation(audio()))
                .await
                .unwrap()
                .stream
                .collect::<Vec<_>>()
                .await
        };
        let ((), parts) = tokio::join!(serve, client);
        assert!(matches!(
            parts.as_slice(),
            [Translation::StreamStart { .. }, Translation::Error { .. }]
        ));
    }
}
