//! Multipart transport probe used by provider upload parity tests.

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::files::UploadData;
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub(crate) struct StreamingUploadProbe {
    first_received: Arc<AtomicBool>,
}
impl StreamingUploadProbe {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            first_received: Arc::new(AtomicBool::new(false)),
        })
    }
    pub(crate) fn data(&self) -> UploadData {
        let received = Arc::clone(&self.first_received);
        UploadData::Stream(Box::pin(futures_util::stream::unfold(0, move |step| {
            let received = Arc::clone(&received);
            async move {
                match step {
                    0 => Some((Ok(Bytes::from_static(b"first")), 1)),
                    1 => {
                        assert!(
                            received.load(Ordering::SeqCst),
                            "provider collected the second chunk before sending the first"
                        );
                        Some((Ok(Bytes::from_static(b"second")), 2))
                    }
                    _ => None,
                }
            }
        })))
    }
}
impl HttpTransport for StreamingUploadProbe {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let mut body = request.body.into_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = body.next().await {
                let chunk = chunk?;
                if chunk.as_ref() == b"first" {
                    self.first_received.store(true, Ordering::SeqCst);
                }
                bytes.extend_from_slice(&chunk);
            }
            assert!(String::from_utf8_lossy(&bytes).contains("firstsecond"));
            Ok(HttpResponse::from_bytes(
                http::StatusCode::OK,
                Headers::new().with("content-type", "application/json"),
                Bytes::from_static(br#"{"id":"file-stream"}"#),
            ))
        })
    }
}
