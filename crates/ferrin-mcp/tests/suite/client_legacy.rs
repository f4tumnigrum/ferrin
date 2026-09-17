//! Legacy (`initialize`) negotiation over a mock transport.

use std::sync::Arc;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpError;
use ferrin_mcp::RequestOptions;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::protocol::LATEST_LEGACY_PROTOCOL_VERSION;
use ferrin_mcp::protocol::ProtocolEra;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::TransportCapabilities;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::MockTransport;
use super::common::call_tool_result;
use super::common::config;
use super::common::connect;
use super::common::discovery_capabilities;
use super::common::initialize_result;
use super::common::legacy_transport;
use super::common::reply;
use super::common::reply_error;

#[tokio::test]
async fn unknown_discover_method_falls_back_to_initialize() {
    let transport = legacy_transport(LATEST_LEGACY_PROTOCOL_VERSION, |request| {
        match request.method.as_str() {
            "tools/call" => Ok(call_tool_result("legacy ok")),
            _ => Err((-32601, "method not found".to_owned())),
        }
    });
    let client = connect(Arc::clone(&transport)).await;
    assert_eq!(client.protocol_era(), ProtocolEra::Legacy);
    assert_eq!(
        client.protocol_version().as_deref(),
        Some(LATEST_LEGACY_PROTOCOL_VERSION)
    );
    assert_eq!(client.server_info().unwrap().name, "legacy-server");
    assert_eq!(
        client.instructions().as_deref(),
        Some("Legacy instructions.")
    );
    assert_eq!(
        transport.methods(),
        vec!["server/discover", "initialize", "notifications/initialized"]
    );
    let initialize = transport.requests("initialize").remove(0);
    insta::assert_json_snapshot!("legacy_initialize_request", initialize.params);
    let result = client
        .call_tool("echo", None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(result.text(), "legacy ok");
    let call = transport.requests("tools/call").remove(0);
    assert!(!call.params.unwrap().contains_key("_meta"));
}

#[tokio::test]
async fn discovery_is_skipped_when_the_transport_does_not_support_it() {
    let transport = MockTransport::new(TransportCapabilities::default(), |message| {
        match message.method() {
            Some("initialize") => Ok(vec![reply(message, initialize_result("2025-03-26"))]),
            Some("server/discover") => panic!("discover must not be sent"),
            _ => Ok(Vec::new()),
        }
    });
    let client = connect(Arc::clone(&transport)).await;
    assert_eq!(client.protocol_version().as_deref(), Some("2025-03-26"));
    assert_eq!(
        transport.methods(),
        vec!["initialize", "notifications/initialized"]
    );
}

#[tokio::test]
async fn discovery_can_be_disabled_in_the_config() {
    let transport = legacy_transport(LATEST_LEGACY_PROTOCOL_VERSION, |_| Ok(json!({})));
    let client = McpClient::connect(config(Arc::clone(&transport)).protocol_discovery(false))
        .await
        .unwrap();
    assert_eq!(client.protocol_era(), ProtocolEra::Legacy);
    assert_eq!(
        transport.methods(),
        vec!["initialize", "notifications/initialized"]
    );
}

#[tokio::test]
async fn modern_error_codes_abort_negotiation() {
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![JsonRpcMessage::error(
                message.id().cloned(),
                -32022,
                "unsupported protocol version",
                Some(json!({"supported": ["2027-01-01"], "requested": "2026-07-28"})),
            )]),
            Some("initialize") => panic!("initialize must not be sent after a modern error"),
            _ => Ok(Vec::new()),
        });
    let error = McpClient::connect(config(Arc::clone(&transport)))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(-32022));
    assert!(transport.is_closed());
}

#[tokio::test]
async fn unsupported_server_versions_are_rejected() {
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![reply_error(message, -32601, "nope")]),
            Some("initialize") => Ok(vec![reply(message, initialize_result("1999-01-01"))]),
            _ => Ok(Vec::new()),
        });
    let error = McpClient::connect(config(transport)).await.unwrap_err();
    assert!(matches!(error, McpError::Protocol { ref message } if message.contains("1999-01-01")));
}

#[tokio::test]
async fn discover_listing_only_legacy_versions_runs_initialize() {
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![reply(
                message,
                json!({
                    "resultType": "complete",
                    "supportedVersions": ["2025-06-18"],
                    "capabilities": {}
                }),
            )]),
            Some("initialize") => Ok(vec![reply(message, initialize_result("2025-06-18"))]),
            _ => Ok(Vec::new()),
        });
    let client = connect(Arc::clone(&transport)).await;
    assert_eq!(client.protocol_version().as_deref(), Some("2025-06-18"));
    assert_eq!(
        transport.methods(),
        vec!["server/discover", "initialize", "notifications/initialized"]
    );
    assert_eq!(transport.protocol_version().as_deref(), Some("2025-06-18"));
}

#[tokio::test]
async fn unusable_discovery_results_fall_back_to_initialize() {
    for result in [
        json!({"resultType": "complete", "supportedVersions": ["2030-01-01"], "capabilities": {}}),
        json!({"resultType": "complete", "capabilities": {}}),
        json!({"supportedVersions": ["2026-07-28"], "capabilities": {}}),
    ] {
        let transport = MockTransport::new(discovery_capabilities(), move |message| match message
            .method()
        {
            Some("server/discover") => Ok(vec![reply(message, result.clone())]),
            Some("initialize") => Ok(vec![reply(message, initialize_result("2025-06-18"))]),
            _ => Ok(Vec::new()),
        });
        let client = connect(Arc::clone(&transport)).await;
        assert_eq!(client.protocol_version().as_deref(), Some("2025-06-18"));
        assert_eq!(
            transport.methods(),
            vec!["server/discover", "initialize", "notifications/initialized"]
        );
    }
}
