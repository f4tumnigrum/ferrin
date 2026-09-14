//! Legacy HTTP+SSE transport against the fixture server.

use std::time::Duration;

use ferrin_mcp::McpError;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::transport::CloseOptions;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::SseTransport;
use ferrin_mcp::transport::SseTransportConfig;
use ferrin_mcp::transport::TransportEvent;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::local_policy;

async fn transport(server: &FixtureServer) -> SseTransport {
    let url = server.url().join("/sse").unwrap();
    SseTransport::new(SseTransportConfig::new(url).url_policy(local_policy())).unwrap()
}

#[tokio::test]
async fn endpoint_event_selects_the_post_target_and_messages_flow_back() {
    let server = FixtureServer::start().await.unwrap();
    server.mount_once(
        Method::GET,
        "/sse",
        Fixture::sse([
            "event: endpoint\ndata: /messages?session=abc".to_owned(),
            format!(
                "event: message\ndata: {}",
                json!({"jsonrpc": "2.0", "id": 5, "result": {"pong": true}})
            ),
        ])
        .hold_open(),
    );
    server.mount(
        Method::POST,
        "/messages",
        Fixture::complete(StatusCode::ACCEPTED, "text/plain", "Accepted"),
    );
    let transport = transport(&server).await;
    let mut incoming = transport.incoming();
    transport.start().await.unwrap();
    assert_eq!(
        transport.endpoint().unwrap().as_str(),
        server.url().join("/messages?session=abc").unwrap().as_str()
    );
    transport.set_protocol_version(Some("2024-11-05"));
    transport
        .send(
            JsonRpcMessage::request(5, "ping", None),
            SendOptions::default(),
        )
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), incoming.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        first,
        TransportEvent::Message(JsonRpcMessage::Response(response)) if response.id == 5.into()
    ));
    transport.close(CloseOptions::default()).await.unwrap();
    let last = tokio::time::timeout(Duration::from_secs(5), incoming.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(last, TransportEvent::Closed));
    let post = server
        .received()
        .into_iter()
        .find(|request| request.method == Method::POST)
        .unwrap();
    assert_eq!(post.path, "/messages");
    assert_eq!(post.query.as_deref(), Some("session=abc"));
    assert_eq!(post.header("content-type"), Some("application/json"));
    assert_eq!(post.header("mcp-protocol-version"), Some("2024-11-05"));
    assert_eq!(post.body_json().unwrap()["id"], json!(5));
    assert!(matches!(
        transport
            .send(
                JsonRpcMessage::request(6, "ping", None),
                SendOptions::default()
            )
            .await,
        Err(McpError::Closed)
    ));
}

#[tokio::test]
async fn cross_origin_endpoints_are_rejected() {
    let server = FixtureServer::start().await.unwrap();
    server.mount_once(
        Method::GET,
        "/sse",
        Fixture::sse(["event: endpoint\ndata: https://other.example/messages"]),
    );
    let transport = transport(&server).await;
    let error = transport.start().await.unwrap_err();
    assert!(
        matches!(error, McpError::Protocol { ref message } if message.contains("origin")),
        "{error:?}"
    );
}

#[tokio::test]
async fn failed_stream_requests_surface_the_status() {
    let server = FixtureServer::start().await.unwrap();
    server.mount_once(
        Method::GET,
        "/sse",
        Fixture::complete(StatusCode::NOT_FOUND, "text/plain", "nope"),
    );
    let transport = transport(&server).await;
    let error = transport.start().await.unwrap_err();
    assert_eq!(error.status_code(), Some(404));
    let unauthorized = FixtureServer::start().await.unwrap();
    unauthorized.mount_once(
        Method::GET,
        "/sse",
        Fixture::complete(StatusCode::UNAUTHORIZED, "text/plain", ""),
    );
    let transport = self::transport(&unauthorized).await;
    assert!(matches!(
        transport.start().await,
        Err(McpError::Unauthorized)
    ));
}

#[tokio::test]
async fn an_ended_stream_closes_the_transport() {
    let server = FixtureServer::start().await.unwrap();
    server.mount_once(
        Method::GET,
        "/sse",
        Fixture::sse(["event: endpoint\ndata: /messages"]),
    );
    let transport = transport(&server).await;
    let mut incoming = transport.incoming();
    transport.start().await.unwrap();
    let mut events = Vec::new();
    for _ in 0..2 {
        events.push(
            tokio::time::timeout(Duration::from_secs(5), incoming.next())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert!(matches!(
        &events[0],
        TransportEvent::Error(error) if error.to_string().contains("event stream ended")
    ));
    assert!(matches!(events[1], TransportEvent::Closed));
    assert!(matches!(
        transport
            .send(
                JsonRpcMessage::request(6, "ping", None),
                SendOptions::default()
            )
            .await,
        Err(McpError::Closed)
    ));
}
