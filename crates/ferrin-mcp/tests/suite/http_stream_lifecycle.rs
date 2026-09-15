//! Drop-aware in-memory HTTP bodies exercise request stream ownership.

use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::transport::CloseOptions;
use ferrin_mcp::transport::HttpTransport;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport as ProviderHttpTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_spec::Headers;
use futures_util::future::BoxFuture;
use http::StatusCode;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::local_policy;

struct DropNotice(Arc<Notify>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

struct PendingBodyTransport {
    started: Arc<Notify>,
    dropped: Arc<Notify>,
}

impl ProviderHttpTransport for PendingBodyTransport {
    fn execute(
        &self,
        _request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let started = Arc::clone(&self.started);
        let notice = DropNotice(Arc::clone(&self.dropped));
        Box::pin(async move {
            let body = futures_util::stream::poll_fn(move |_| {
                let _keep_alive = &notice;
                started.notify_one();
                Poll::Pending
            });
            Ok(HttpResponse::from_stream(
                StatusCode::OK,
                Headers::new().with("content-type", "text/event-stream"),
                Box::pin(body),
            ))
        })
    }
}

#[tokio::test]
async fn request_stream_body_is_released_on_request_cancel_close_and_drop() {
    for action in ["request", "close", "drop"] {
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(Notify::new());
        let http = Arc::new(PendingBodyTransport {
            started: Arc::clone(&started),
            dropped: Arc::clone(&dropped),
        });
        let config = HttpTransportConfig::new(Url::parse("http://127.0.0.1/mcp").unwrap())
            .url_policy(local_policy())
            .transport(http);
        let transport = HttpTransport::new(config).unwrap();
        transport.start().await.unwrap();
        let cancellation = CancellationToken::new();
        transport
            .send(
                JsonRpcMessage::request(1, "ping", None),
                SendOptions {
                    cancellation: Some(cancellation.clone()),
                    ..SendOptions::default()
                },
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        match action {
            "request" => cancellation.cancel(),
            "close" => transport.close(CloseOptions::default()).await.unwrap(),
            "drop" => drop(transport),
            _ => unreachable!(),
        }
        tokio::time::timeout(Duration::from_secs(5), dropped.notified())
            .await
            .unwrap();
    }
}

#[derive(Clone, Copy)]
enum ReplyBody {
    Empty,
    Notification,
    WrongId,
    Malformed,
    BodyError,
    Complete,
    RpcError,
}

struct ReplyTransport {
    calls: std::sync::atomic::AtomicUsize,
    reply: ReplyBody,
}

impl ProviderHttpTransport for ReplyTransport {
    fn execute(
        &self,
        _request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        use bytes::Bytes;
        use ferrin_provider_util::http::TransportErrorKind;
        use serde_json::json;
        use std::sync::atomic::Ordering;
        let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            if first {
                let body =
                    json!({"jsonrpc": "2.0", "id": 1, "result": super::common::discover_result()})
                        .to_string();
                return Ok(HttpResponse::from_bytes(
                    StatusCode::OK,
                    Headers::new().with("content-type", "application/json"),
                    body.into(),
                ));
            }
            let chunks = match self.reply {
                ReplyBody::Empty => Vec::new(),
                ReplyBody::Notification => vec![Ok(Bytes::from_static(b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n"))],
                ReplyBody::WrongId => vec![Ok(Bytes::from_static(b"data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}\n\n"))],
                ReplyBody::Malformed => vec![Ok(Bytes::from_static(b"data: invalid-json\n\n"))],
                ReplyBody::BodyError => vec![Err(TransportError::new(TransportErrorKind::Body, "injected body failure"))],
                ReplyBody::Complete => vec![Ok(Bytes::from_static(b"data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"resultType\":\"complete\"}}\n\n"))],
                ReplyBody::RpcError => vec![Ok(Bytes::from_static(b"data: {\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32603,\"message\":\"injected failure\"}}\n\n"))],
            };
            Ok(HttpResponse::from_stream(
                StatusCode::OK,
                Headers::new().with("content-type", "text/event-stream"),
                Box::pin(futures_util::stream::iter(chunks)),
            ))
        })
    }
}

#[tokio::test]
async fn request_stream_without_matching_response_fails_without_request_timeout() {
    use ferrin_mcp::McpClient;
    use ferrin_mcp::McpClientConfig;
    use ferrin_mcp::McpError;
    use ferrin_mcp::RequestOptions;
    use ferrin_mcp::transport::TransportConfig;
    for reply in [
        ReplyBody::Empty,
        ReplyBody::Notification,
        ReplyBody::WrongId,
        ReplyBody::Malformed,
        ReplyBody::BodyError,
        ReplyBody::Complete,
        ReplyBody::RpcError,
    ] {
        let http = Arc::new(ReplyTransport {
            calls: std::sync::atomic::AtomicUsize::new(0),
            reply,
        });
        let config = HttpTransportConfig::new(Url::parse("http://127.0.0.1/mcp").unwrap())
            .url_policy(local_policy())
            .transport(http);
        let client = McpClient::connect(McpClientConfig::new(TransportConfig::Http(config)))
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.ping(RequestOptions::default()),
        )
        .await
        .expect("request SSE must resolve its waiter at EOF");
        match reply {
            ReplyBody::Complete => result.unwrap(),
            ReplyBody::RpcError => assert!(matches!(
                result,
                Err(McpError::JsonRpc { code: -32603, .. })
            )),
            ReplyBody::BodyError => assert!(matches!(result, Err(McpError::Transport(_)))),
            _ => assert!(
                matches!(result, Err(McpError::Protocol { .. })),
                "{result:?}"
            ),
        }
        client.close().await.unwrap();
    }
}
