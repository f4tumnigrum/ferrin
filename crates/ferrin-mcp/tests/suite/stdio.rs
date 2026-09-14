//! Stdio transport against a Python test server.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpClientConfig;
use ferrin_mcp::McpError;
use ferrin_mcp::RequestOptions;
use ferrin_mcp::ToolsOptions;
use ferrin_mcp::elicitation_handler;
use ferrin_mcp::protocol::ElicitResult;
use ferrin_mcp::protocol::LATEST_LEGACY_PROTOCOL_VERSION;
use ferrin_mcp::protocol::ProtocolEra;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::StdioConfig;
use ferrin_mcp::transport::StdioStderr;
use ferrin_mcp::transport::StdioTransport;
use ferrin_mcp::transport::TransportConfig;
use pretty_assertions::assert_eq;
use serde_json::json;

const SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stdio/echo_server.py"
);

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn stdio_config() -> StdioConfig {
    StdioConfig::new(python())
        .args([SERVER])
        .env("FERRIN_TEST_ENV", "from-test")
        .stderr(StdioStderr::Null)
}

#[tokio::test]
async fn negotiates_lists_and_calls_tools_over_stdio() {
    let notifications = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&notifications);
    let config = McpClientConfig::new(TransportConfig::Stdio(stdio_config()))
        .default_request_timeout(Duration::from_secs(20))
        .on_notification(Arc::new(move |notification| {
            sink.lock().unwrap().push(notification.method.clone());
        }));
    let client = McpClient::connect(config).await.unwrap();
    assert_eq!(client.protocol_era(), ProtocolEra::Legacy);
    assert_eq!(
        client.protocol_version().as_deref(),
        Some(LATEST_LEGACY_PROTOCOL_VERSION)
    );
    assert_eq!(client.server_info().unwrap().name, "python-stdio");
    assert_eq!(client.instructions().as_deref(), Some("stdio instructions"));
    let tools = client.tools(ToolsOptions::default()).await.unwrap();
    assert_eq!(tools.len(), 4);
    let echo = client
        .call_tool(
            "echo",
            json!({"text": "hi"}).as_object().cloned(),
            RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(echo.text(), "hi");
    let env = client
        .call_tool("env", None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(env.text(), "from-test");
    let failed = client
        .call_tool("fail", None, RequestOptions::default())
        .await
        .unwrap();
    assert!(failed.is_error);
    client.ping(RequestOptions::default()).await.unwrap();
    assert_eq!(
        *notifications.lock().unwrap(),
        vec!["notifications/message"]
    );
    client.close().await.unwrap();
    assert!(matches!(
        client.ping(RequestOptions::default()).await,
        Err(McpError::Closed)
    ));
}

#[tokio::test]
async fn server_requests_are_answered_over_stdio() {
    let handler = elicitation_handler(|request| async move {
        assert_eq!(request.message, "What is your name?");
        Ok(ElicitResult::accept(
            json!({"name": "Ferrin"}).as_object().cloned().unwrap(),
        ))
    });
    let config = McpClientConfig::new(TransportConfig::Stdio(stdio_config()))
        .default_request_timeout(Duration::from_secs(20))
        .elicitation_handler(handler);
    let client = McpClient::connect(config).await.unwrap();
    let asked = client
        .call_tool("ask", None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(
        asked.text(),
        r#"{"action": "accept", "content": {"name": "Ferrin"}}"#
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn missing_commands_fail_to_start() {
    let transport = StdioTransport::new(StdioConfig::new("ferrin-definitely-missing-binary"));
    let error = transport.start().await.unwrap_err();
    assert!(matches!(error, McpError::Io(_)), "{error:?}");
    assert_eq!(transport.pid(), None);
}
