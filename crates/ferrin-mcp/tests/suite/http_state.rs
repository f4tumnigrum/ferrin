//! Legacy HTTP session and inbound stream transitions.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::transport::HttpTransport;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::TransportEvent;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpResponse;
use ferrin_provider_util::TransportError;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use tokio::sync::Notify;
use url::Url;

use super::common::local_policy;

#[derive(Default)]
struct QueueHttp {
    get: Mutex<VecDeque<HttpResponse>>,
    post: Mutex<VecDeque<HttpResponse>>,
    get_finished: Notify,
}

impl ferrin_provider_util::HttpTransport for QueueHttp {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let response = if request.method == Method::GET {
                let response = self.get.lock().unwrap().pop_front().unwrap();
                self.get_finished.notify_one();
                response
            } else {
                self.post.lock().unwrap().pop_front().unwrap()
            };
            Ok(response)
        })
    }
}

fn response(status: StatusCode, headers: Headers, body: &'static str) -> HttpResponse {
    HttpResponse::from_bytes(status, headers, Bytes::from_static(body.as_bytes()))
}

async fn transport(
    http: Arc<QueueHttp>,
    configure: impl FnOnce(HttpTransportConfig) -> HttpTransportConfig,
) -> HttpTransport {
    let config = HttpTransportConfig::new(Url::parse("http://127.0.0.1:9/mcp").unwrap())
        .url_policy(local_policy())
        .transport(http)
        .terminate_session_on_close(false);
    let transport = HttpTransport::new(configure(config)).unwrap();
    transport.start().await.unwrap();
    transport.set_protocol_version(Some("2025-11-25"));
    transport
}

#[tokio::test]
async fn accepted_notifications_retry_an_inbound_stream_after_405() {
    let http = Arc::new(QueueHttp::default());
    http.get.lock().unwrap().extend([
        response(StatusCode::METHOD_NOT_ALLOWED, Headers::new(), ""),
        response(StatusCode::OK, Headers::new().with("content-type", "text/event-stream").with("mcp-session-id", "new"),
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n\n"),
    ]);
    for _ in 0..2 {
        http.post
            .lock()
            .unwrap()
            .push_back(response(StatusCode::ACCEPTED, Headers::new(), ""));
    }
    let transport = transport(Arc::clone(&http), |config| config).await;
    let mut incoming = transport.incoming();
    transport
        .send(
            JsonRpcMessage::notification("notifications/initialized", None),
            SendOptions::default(),
        )
        .await
        .unwrap();
    http.get_finished.notified().await;
    transport
        .send(
            JsonRpcMessage::notification("notifications/progress", None),
            SendOptions::default(),
        )
        .await
        .unwrap();
    assert!(matches!(
        incoming.next().await,
        Some(TransportEvent::Message(JsonRpcMessage::Notification(_)))
    ));
    assert_eq!(
        (transport.session_id(), http.get.lock().unwrap().len()),
        (Some("new".into()), 0)
    );
}

#[tokio::test]
async fn session_expiry_applies_to_the_sent_session_and_reports_changes() {
    for inbound in [false, true] {
        for changed in [false, true] {
            let http = Arc::new(QueueHttp::default());
            let headers = if changed {
                Headers::new().with("mcp-session-id", "replacement")
            } else {
                Headers::new()
            };
            let expired_response = response(StatusCode::NOT_FOUND, headers, "gone");
            if inbound {
                http.get.lock().unwrap().push_back(expired_response);
                http.post.lock().unwrap().push_back(response(
                    StatusCode::ACCEPTED,
                    Headers::new(),
                    "",
                ));
            } else {
                http.post.lock().unwrap().push_back(expired_response);
            }
            let changes = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
            let expirations = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
            let transport = transport(http, {
                let changes = Arc::clone(&changes);
                let expirations = Arc::clone(&expirations);
                move |config| {
                    config
                        .session_id("original")
                        .on_session_id_change(Arc::new(move |id| {
                            changes.lock().unwrap().push(id.map(str::to_owned))
                        }))
                        .on_session_expired(Arc::new(move |id| {
                            expirations.lock().unwrap().push(id.map(str::to_owned))
                        }))
                }
            })
            .await;
            if inbound {
                let mut incoming = transport.incoming();
                transport
                    .send(
                        JsonRpcMessage::notification("notifications/initialized", None),
                        SendOptions::default(),
                    )
                    .await
                    .unwrap();
                assert!(matches!(
                    incoming.next().await,
                    Some(TransportEvent::Error(_))
                ));
            } else {
                assert!(
                    transport
                        .send(
                            JsonRpcMessage::request(1, "ping", None),
                            SendOptions::default()
                        )
                        .await
                        .is_err()
                );
            }
            let current = changed.then(|| "replacement".to_owned());
            assert_eq!(
                (
                    transport.session_id(),
                    changes.lock().unwrap().clone(),
                    expirations.lock().unwrap().clone()
                ),
                (
                    current.clone(),
                    vec![current],
                    vec![Some("original".to_owned())]
                ),
            );
        }
    }
}
