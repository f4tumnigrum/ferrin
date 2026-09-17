use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::MultipartForm;
use ferrin_provider_util::http::MultipartPart;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::ReqwestTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::http::TransportErrorKind;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use futures_util::stream;
use pretty_assertions::assert_eq;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

async fn collect(body: RequestBody) -> Result<Bytes, TransportError> {
    let mut output = Vec::new();
    let mut chunks = body.into_stream();
    while let Some(chunk) = chunks.try_next().await? {
        output.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(output))
}

#[tokio::test]
async fn multipart_preserves_wire_encoding_without_polling_files_early() {
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let chunks = stream::iter([
        Ok(Bytes::from_static(b"abc")),
        Ok(Bytes::from_static(b"def")),
    ])
    .inspect(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let buffered = MultipartForm::with_boundary("fixed")
        .field("purpose", "batch")
        .file(
            "file",
            Some("a\"\r\n.txt".into()),
            Some("text/plain\r\n".into()),
            "abcdef".into(),
        )
        .encode()
        .unwrap();
    let form = MultipartForm::with_boundary("fixed")
        .field("purpose", "batch")
        .file_stream(
            "file",
            Some("a\"\r\n.txt".into()),
            Some("text/plain\r\n".into()),
            Box::pin(chunks),
        );
    assert!(form.encode().is_err());
    assert_eq!(
        (polls.load(Ordering::SeqCst), form.content_length()),
        (0, None)
    );
    let actual = collect(RequestBody::Multipart(form)).await.unwrap();
    assert_eq!((actual, polls.load(Ordering::SeqCst)), (buffered, 2));
}

#[tokio::test]
async fn known_lengths_match_encoded_multipart_and_reject_short_or_long_streams() {
    for length in [2, 3, 4] {
        let form = MultipartForm::with_boundary("fixed").part(MultipartPart::StreamFile {
            name: "file".into(),
            filename: None,
            media_type: None,
            data: Box::pin(stream::once(std::future::ready(Ok("abc".into())))),
            content_length: Some(length),
        });
        let declared = form.content_length();
        let result = collect(RequestBody::Multipart(form)).await;
        if length == 3 {
            let expected = MultipartForm::with_boundary("fixed")
                .file("file", None, None, "abc".into())
                .encode()
                .unwrap();
            assert_eq!(
                (result.unwrap(), declared),
                (expected.clone(), Some(expected.len() as u64))
            );
        } else {
            assert_eq!(result.unwrap_err().kind, TransportErrorKind::Body);
        }
    }
}

#[tokio::test]
async fn input_errors_end_multipart_before_later_files_are_polled() {
    let later_polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&later_polls);
    let later = stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok(Bytes::from_static(b"unreachable"))))
    });
    let form = MultipartForm::with_boundary("fixed")
        .file_stream(
            "first",
            None,
            None,
            Box::pin(stream::iter([
                Ok("prefix".into()),
                Err(TransportError::new(
                    TransportErrorKind::Body,
                    "upload input failed",
                )),
                Ok("unreachable".into()),
            ])),
        )
        .file_stream("later", None, None, Box::pin(later));
    let mut stream = form.into_stream();
    assert!(stream.next().await.unwrap().is_ok());
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        Bytes::from_static(b"prefix")
    );
    assert_eq!(
        stream.next().await.unwrap().unwrap_err().kind,
        TransportErrorKind::Body
    );
    assert!(stream.next().await.is_none());
    assert_eq!(later_polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn transport_sends_the_first_chunk_before_the_producer_finishes() {
    for declared in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let first_received = Arc::new(Notify::new());
        let gate = Arc::clone(&first_received);
        let mut server = JoinSet::new();
        server.spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if text.contains("first-upload-chunk") {
                    gate.notify_one();
                }
                if text.contains("--fixed--\r\n") {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let file = stream::once(std::future::ready(Ok(Bytes::from_static(
            b"first-upload-chunk",
        ))))
        .chain(stream::once(async move {
            first_received.notified().await;
            Ok(Bytes::from_static(b"second-upload-chunk"))
        }));
        let form = MultipartForm::with_boundary("fixed").part(MultipartPart::StreamFile {
            name: "file".into(),
            filename: Some("data.txt".into()),
            media_type: Some("text/plain".into()),
            data: Box::pin(file),
            content_length: declared
                .then_some(b"first-upload-chunksecond-upload-chunk".len() as u64),
        });
        let length = form.content_length();
        let request = HttpRequest::post(Url::parse(&format!("http://{address}/upload")).unwrap())
            .with_body(RequestBody::Multipart(form));
        let response = ReqwestTransport::new()
            .unwrap()
            .execute(request)
            .await
            .unwrap();
        assert_eq!(response.status, http::StatusCode::OK);
        let request = server
            .join_next()
            .await
            .unwrap()
            .unwrap()
            .to_ascii_lowercase();
        let framing = length.map_or_else(
            || "transfer-encoding: chunked".into(),
            |length| format!("content-length: {length}"),
        );
        assert!(request.contains(&framing), "{request}");
    }
}

struct Dropped(Arc<Notify>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test]
async fn cancellation_and_request_drop_release_pending_input_streams() {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let polled = Arc::new(Notify::new());
        let observed = Arc::clone(&polled);
        let dropped = Arc::new(Notify::new());
        let guard = Dropped(Arc::clone(&dropped));
        let input = stream::poll_fn(move |_| {
            let _keep_alive = &guard;
            observed.notify_one();
            Poll::Pending
        });
        let cancellation = CancellationToken::new();
        let request = HttpRequest::post(Url::parse(&format!("http://{address}/upload")).unwrap())
            .with_body(RequestBody::Stream {
                content_type: "application/octet-stream".into(),
                data: Box::pin(input),
                content_length: None,
            })
            .with_cancellation(cancellation.clone());
        let transport = ReqwestTransport::new().unwrap();
        {
            let pending = transport.execute(request);
            tokio::pin!(pending);
            tokio::select! {
                biased;
                result = &mut pending => panic!("pending input unexpectedly completed: {result:?}"),
                () = polled.notified() => {}
            }
            if cancel {
                cancellation.cancel();
                assert_eq!(
                    pending.await.unwrap_err().kind,
                    TransportErrorKind::Cancelled
                );
            }
        }
        dropped.notified().await;
    }
}
