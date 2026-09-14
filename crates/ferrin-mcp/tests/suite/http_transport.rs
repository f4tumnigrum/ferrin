//! Streamable HTTP transport against the fixture server.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_mcp::McpError;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::protocol::LATEST_PROTOCOL_VERSION;
use ferrin_mcp::transport::CloseOptions;
use ferrin_mcp::transport::HttpTransport;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::RedirectMode;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::TransportEvent;
use ferrin_spec::Headers;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use url::Url;

use super::common::local_policy;

struct Harness {
    server: FixtureServer,
    transport: HttpTransport,
    incoming: BoxStream<'static, TransportEvent>,
}

impl Harness {
    async fn start(configure: impl FnOnce(HttpTransportConfig) -> HttpTransportConfig) -> Self {
        let server = FixtureServer::start().await.unwrap();
        let url = server.url().join("/mcp").unwrap();
        let config = configure(HttpTransportConfig::new(url).url_policy(local_policy()));
        let transport = HttpTransport::new(config).unwrap();
        transport.start().await.unwrap();
        let incoming = transport.incoming();
        Self {
            server,
            transport,
            incoming,
        }
    }

    async fn next(&mut self) -> TransportEvent {
        tokio::time::timeout(Duration::from_secs(5), self.incoming.next())
            .await
            .expect("timed out waiting for a transport event")
            .expect("event stream ended")
    }

    async fn send(&self, message: JsonRpcMessage) -> Result<(), McpError> {
        self.transport.send(message, SendOptions::default()).await
    }
}

fn response_json(id: i64, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn request(id: i64, method: &str) -> JsonRpcMessage {
    JsonRpcMessage::request(id, method, json!({"name": "echo"}).as_object().cloned())
}

#[tokio::test]
async fn json_responses_are_delivered_with_standard_headers() {
    let mut harness = Harness::start(|config| config).await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json(&response_json(1, json!({"ok": true}))),
    );
    harness.transport.set_protocol_version(Some("2025-11-25"));
    harness.send(request(1, "ping")).await.unwrap();
    let event = harness.next().await;
    let TransportEvent::Message(JsonRpcMessage::Response(response)) = event else {
        panic!("unexpected event {event:?}");
    };
    assert_eq!(Value::Object(response.result), json!({"ok": true}));
    let received = harness.server.received().remove(0);
    assert_eq!(
        received.header("accept"),
        Some("application/json, text/event-stream")
    );
    assert_eq!(received.header("content-type"), Some("application/json"));
    assert_eq!(received.header("mcp-protocol-version"), Some("2025-11-25"));
    assert!(
        received
            .header("user-agent")
            .unwrap()
            .contains("ferrin-mcp/")
    );
    assert_eq!(received.header("mcp-method"), None);
    assert_eq!(received.body_json().unwrap()["method"], json!("ping"));
}

#[tokio::test]
async fn event_stream_responses_are_read_in_the_background() {
    let mut harness = Harness::start(|config| config).await;
    let events = [
        format!(
            "event: message\ndata: {}",
            json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1}})
        ),
        format!(
            "event: message\ndata: {}",
            response_json(2, json!({"done": true}))
        ),
    ];
    harness
        .server
        .mount_once(Method::POST, "/mcp", Fixture::sse(events));
    harness.send(request(2, "tools/call")).await.unwrap();
    assert!(matches!(
        harness.next().await,
        TransportEvent::Message(JsonRpcMessage::Notification(notification)) if notification.method == "notifications/progress"
    ));
    assert!(matches!(
        harness.next().await,
        TransportEvent::Message(JsonRpcMessage::Response(response)) if response.result["done"] == json!(true)
    ));
}

#[tokio::test]
async fn legacy_sessions_capture_the_session_id_and_skip_it_on_initialize() {
    let changes = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let sink = Arc::clone(&changes);
    let mut harness = Harness::start(|config| {
        config
            .session_id("stale")
            .on_session_id_change(Arc::new(move |id| {
                sink.lock().unwrap().push(id.map(str::to_owned))
            }))
    })
    .await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json(&response_json(1, json!({}))).with_header("mcp-session-id", "sess-1"),
    );
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json(&response_json(2, json!({}))),
    );
    harness.send(request(1, "initialize")).await.unwrap();
    let _ = harness.next().await;
    harness.transport.set_protocol_version(Some("2025-11-25"));
    harness.send(request(2, "tools/list")).await.unwrap();
    let _ = harness.next().await;
    let received = harness.server.received();
    assert_eq!(received[0].header("mcp-session-id"), None);
    assert_eq!(received[1].header("mcp-session-id"), Some("sess-1"));
    assert_eq!(harness.transport.session_id().as_deref(), Some("sess-1"));
    assert_eq!(*changes.lock().unwrap(), vec![Some("sess-1".to_owned())]);
}

#[tokio::test]
async fn accepted_notifications_return_without_events_and_start_the_inbound_stream() {
    let mut harness = Harness::start(|config| config).await;
    harness.transport.set_protocol_version(Some("2025-11-25"));
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::ACCEPTED, "text/plain", ""),
    );
    let inbound = format!(
        "id: evt-1\nevent: message\ndata: {}",
        json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
    );
    harness
        .server
        .mount_once(Method::GET, "/mcp", Fixture::sse([inbound]));
    harness
        .send(JsonRpcMessage::notification(
            "notifications/initialized",
            None,
        ))
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        TransportEvent::Message(JsonRpcMessage::Notification(notification))
            if notification.method == "notifications/tools/list_changed"
    ));
    let received = harness.server.received();
    assert_eq!(received.len(), 2);
    assert_eq!(received[1].method, Method::GET);
    assert_eq!(received[1].header("accept"), Some("text/event-stream"));
}

#[tokio::test]
async fn inbound_stream_405_is_silently_unsupported() {
    let harness = Harness::start(|config| config).await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::ACCEPTED, "text/plain", ""),
    );
    harness.server.mount_once(
        Method::GET,
        "/mcp",
        Fixture::complete(StatusCode::METHOD_NOT_ALLOWED, "text/plain", ""),
    );
    harness
        .send(JsonRpcMessage::notification(
            "notifications/initialized",
            None,
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while harness.server.received_count() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    harness
        .transport
        .close(CloseOptions::default())
        .await
        .unwrap();
    let mut incoming = harness.incoming;
    assert!(matches!(
        incoming.next().await,
        Some(TransportEvent::Closed)
    ));
}

#[tokio::test]
async fn error_bodies_are_delivered_as_json_rpc_errors() {
    let mut harness = Harness::start(|config| config).await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json_status(
            StatusCode::BAD_REQUEST,
            &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "bad request"}}),
        ),
    );
    harness.send(request(9, "tools/list")).await.unwrap();
    let event = harness.next().await;
    let TransportEvent::Message(JsonRpcMessage::Error(error)) = event else {
        panic!("unexpected event {event:?}");
    };
    assert_eq!(error.id, Some(9.into()));
    assert_eq!(error.error.code, -32600);
}

#[tokio::test]
async fn http_failures_map_to_transport_errors() {
    let expired = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let sink = Arc::clone(&expired);
    let harness = Harness::start(|config| {
        config
            .session_id("sess-9")
            .on_session_expired(Arc::new(move |id| {
                sink.lock().unwrap().push(id.map(str::to_owned))
            }))
    })
    .await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::NOT_FOUND, "text/plain", "gone"),
    );
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::UNAUTHORIZED, "text/plain", ""),
    );
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::INTERNAL_SERVER_ERROR, "text/plain", "oops"),
    );
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::OK, "text/plain", "hello"),
    );
    let error = harness.send(request(1, "ping")).await.unwrap_err();
    assert_eq!(error.status_code(), Some(404));
    assert!(error.to_string().contains("session expired"));
    assert_eq!(*expired.lock().unwrap(), vec![Some("sess-9".to_owned())]);
    assert_eq!(harness.transport.session_id(), None);
    let error = harness.send(request(2, "ping")).await.unwrap_err();
    assert!(matches!(error, McpError::Unauthorized));
    let error = harness.send(request(3, "ping")).await.unwrap_err();
    assert_eq!(error.status_code(), Some(500));
    assert!(error.is_retryable_tool_call());
    assert_eq!(
        error.transport_failure().unwrap().response_body.as_deref(),
        Some("oops")
    );
    let error = harness.send(request(4, "ping")).await.unwrap_err();
    assert!(error.to_string().contains("unexpected content type"));
}

#[tokio::test]
async fn redirects_fail_by_default_and_follow_same_origin_when_enabled() {
    let harness = Harness::start(|config| config).await;
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::TEMPORARY_REDIRECT, "text/plain", "")
            .with_header("location", "/mcp2"),
    );
    let error = harness.send(request(1, "ping")).await.unwrap_err();
    assert_eq!(error.status_code(), Some(307));

    let mut following = Harness::start(|config| config.redirect(RedirectMode::Follow)).await;
    following.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::TEMPORARY_REDIRECT, "text/plain", "")
            .with_header("location", "/mcp2"),
    );
    following.server.mount_once(
        Method::POST,
        "/mcp2",
        Fixture::json(&response_json(1, json!({}))),
    );
    following.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::PERMANENT_REDIRECT, "text/plain", "")
            .with_header("location", "https://other.example/mcp"),
    );
    following.send(request(1, "ping")).await.unwrap();
    assert!(matches!(following.next().await, TransportEvent::Message(_)));
    assert_eq!(following.server.received()[1].path, "/mcp2");
    let error = following.send(request(2, "ping")).await.unwrap_err();
    assert!(error.to_string().contains("another origin"));
}

#[tokio::test]
async fn modern_requests_carry_method_name_and_parameter_headers() {
    let mut harness = Harness::start(|config| config.session_id("ignored")).await;
    harness
        .transport
        .set_protocol_version(Some(LATEST_PROTOCOL_VERSION));
    harness.server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json(&response_json(1, json!({"resultType": "complete"}))),
    );
    let options = SendOptions {
        cancellation: None,
        headers: Headers::new().with("Mcp-Param-X-Tenant", "acme"),
    };
    let message = JsonRpcMessage::request(
        1,
        "tools/call",
        json!({"name": "get weather ü"}).as_object().cloned(),
    );
    harness.transport.send(message, options).await.unwrap();
    let _ = harness.next().await;
    let received = harness.server.received().remove(0);
    assert_eq!(received.header("mcp-method"), Some("tools/call"));
    assert_eq!(
        received.header("mcp-name"),
        Some("=?base64?Z2V0IHdlYXRoZXIgw7w=?=")
    );
    assert_eq!(received.header("mcp-param-x-tenant"), Some("acme"));
    assert_eq!(
        received.header("mcp-protocol-version"),
        Some(LATEST_PROTOCOL_VERSION)
    );
    assert_eq!(received.header("mcp-session-id"), None);
}

#[tokio::test]
async fn closing_terminates_legacy_sessions_with_delete() {
    let harness = Harness::start(|config| config.session_id("sess-2")).await;
    harness.transport.set_protocol_version(Some("2025-11-25"));
    harness.server.mount_once(
        Method::DELETE,
        "/mcp",
        Fixture::complete(StatusCode::OK, "text/plain", ""),
    );
    harness
        .transport
        .close(CloseOptions::default())
        .await
        .unwrap();
    let received = harness.server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, Method::DELETE);
    assert_eq!(received[0].header("mcp-session-id"), Some("sess-2"));
    assert!(matches!(
        harness.send(request(1, "ping")).await,
        Err(McpError::Closed)
    ));
    let mut incoming = harness.incoming;
    assert!(matches!(
        incoming.next().await,
        Some(TransportEvent::Closed)
    ));

    let quiet = Harness::start(|config| {
        config
            .session_id("sess-3")
            .terminate_session_on_close(false)
    })
    .await;
    quiet
        .transport
        .close(CloseOptions::default())
        .await
        .unwrap();
    assert_eq!(quiet.server.received_count(), 0);
}

#[tokio::test]
async fn the_url_policy_rejects_disallowed_endpoints() {
    let transport = HttpTransport::new(HttpTransportConfig::new(
        Url::parse("http://127.0.0.1:9/mcp").unwrap(),
    ))
    .unwrap();
    let error = transport.start().await.unwrap_err();
    assert!(matches!(error, McpError::Url(_)), "{error:?}");
    let unstarted = HttpTransport::new(
        HttpTransportConfig::new(Url::parse("https://mcp.example.com/mcp").unwrap())
            .url_policy(local_policy()),
    )
    .unwrap();
    let error = unstarted
        .send(request(1, "ping"), SendOptions::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not been started"));
}
