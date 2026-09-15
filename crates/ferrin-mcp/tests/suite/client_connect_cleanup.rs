//! Failed initialization must survive a stalled session DELETE.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpClientConfig;
use ferrin_mcp::McpError;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::TransportConfig;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_spec::Headers;
use futures_util::future::BoxFuture;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use url::Url;

use super::common::local_policy;

struct DropNotice(Arc<AtomicBool>);
impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct HungDelete {
    send: bool,
    deleted: AtomicBool,
    dropped: Arc<AtomicBool>,
}

impl HttpTransport for HungDelete {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let deleting = request.method == Method::DELETE;
            let notice = deleting.then(|| DropNotice(Arc::clone(&self.dropped)));
            if deleting {
                self.deleted.store(true, Ordering::SeqCst);
                if self.send {
                    let _notice = notice;
                    return std::future::pending().await;
                }
            }
            let body = futures_util::stream::poll_fn(move |_| {
                let _notice = &notice;
                Poll::Pending
            });
            Ok(HttpResponse::from_stream(
                if deleting {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::OK
                },
                Headers::new()
                    .with("mcp-session-id", "synthetic-session")
                    .with("content-type", "application/json"),
                Box::pin(body),
            ))
        })
    }
}

#[tokio::test(start_paused = true)]
async fn failed_initialization_bounds_session_delete_send_and_body() {
    for send in [true, false] {
        let dropped = Arc::new(AtomicBool::new(false));
        let transport = Arc::new(HungDelete {
            send,
            deleted: AtomicBool::new(false),
            dropped: Arc::clone(&dropped),
        });
        let http = HttpTransportConfig::new(Url::parse("http://127.0.0.1/mcp").unwrap())
            .url_policy(local_policy())
            .transport(transport.clone());
        let timeout = Duration::from_millis(10);
        let config = McpClientConfig::new(TransportConfig::Http(http))
            .protocol_discovery(false)
            .initialization_timeout(timeout);
        let started = tokio::time::Instant::now();
        let error = McpClient::connect(config).await.unwrap_err();
        assert!(matches!(error, McpError::Timeout(duration) if duration == timeout));
        assert_eq!(
            (
                started.elapsed(),
                transport.deleted.load(Ordering::SeqCst),
                dropped.load(Ordering::SeqCst)
            ),
            (timeout + Duration::from_secs(1), true, true)
        );
    }
}
