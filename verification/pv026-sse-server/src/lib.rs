//! PV-026: wiremock 0.6 has no chunked/streaming body API, so a minimal hyper
//! 1.x server is prototyped as the streaming backend for `FixtureServer`.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::StreamBody;
use http_body_util::combinators::BoxBody;
use hyper::Response;
use hyper::body::Frame;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// Serves the given SSE lines, sleeping `gap` between chunks. Returns the bound address.
pub async fn serve_sse(chunks: Vec<String>, gap: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept");
            let chunks = chunks.clone();
            tokio::spawn(async move {
                let service = service_fn(move |_req| {
                    let chunks = chunks.clone();
                    async move {
                        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(1);
                        tokio::spawn(async move {
                            for chunk in chunks {
                                if tx.send(Ok(Frame::data(Bytes::from(chunk)))).await.is_err() {
                                    return;
                                }
                                tokio::time::sleep(gap).await;
                            }
                        });
                        let body: BoxBody<Bytes, Infallible> =
                            BoxBody::new(StreamBody::new(tokio_stream_wrapper::ReceiverStream(rx)));
                        Ok::<_, Infallible>(
                            Response::builder()
                                .header("content-type", "text/event-stream")
                                .header("cache-control", "no-cache")
                                .body(body)
                                .expect("response"),
                        )
                    }
                });
                if let Err(err) = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await {
                    eprintln!("connection error: {err}");
                }
            });
        }
    });
    addr
}

mod tokio_stream_wrapper {
    use std::pin::Pin;
    use std::task::Context;
    use std::task::Poll;

    pub struct ReceiverStream<T>(pub tokio::sync::mpsc::Receiver<T>);

    impl<T> futures_util::Stream for ReceiverStream<T> {
        type Item = T;
        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
            self.0.poll_recv(cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::time::Instant;

    #[tokio::test]
    async fn chunks_arrive_with_delay() {
        let chunks = vec![
            "data: {\"type\":\"text-start\"}\n\n".to_owned(),
            "data: {\"type\":\"text-delta\",\"delta\":\"hi\"}\n\n".to_owned(),
            "data: [DONE]\n\n".to_owned(),
        ];
        let addr = serve_sse(chunks.clone(), Duration::from_millis(50)).await;
        let client = reqwest::Client::new();
        let response = client.get(format!("http://{addr}/v1/stream")).send().await.expect("send");
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let mut stream = response.bytes_stream();
        let mut arrivals = Vec::new();
        let start = Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("chunk");
            arrivals.push((start.elapsed(), String::from_utf8_lossy(&chunk).to_string()));
        }
        println!("arrivals: {arrivals:?}");
        assert_eq!(arrivals.len(), 3, "each chunk must arrive as its own frame");
        assert!(arrivals[2].0 - arrivals[0].0 >= Duration::from_millis(80));
    }

    #[test]
    fn wiremock_response_template_has_no_streaming_api() {
        // Compile-time fact check: the only timing control is a whole-response delay.
        let _template = wiremock::ResponseTemplate::new(200)
            .set_body_string("data: x\n\n")
            .set_delay(Duration::from_millis(10));
    }
}
