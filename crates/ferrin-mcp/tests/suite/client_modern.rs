//! Modern (2026-07-28) negotiation and request semantics over a mock
//! transport.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpError;
use ferrin_mcp::RequestOptions;
use ferrin_mcp::elicitation_handler;
use ferrin_mcp::protocol::ElicitResult;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::protocol::LATEST_PROTOCOL_VERSION;
use ferrin_mcp::protocol::ProtocolEra;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::TransportEvent;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::common::MockTransport;
use super::common::call_tool_result;
use super::common::complete;
use super::common::config;
use super::common::connect;
use super::common::discover_result;
use super::common::discovery_capabilities;
use super::common::modern_transport;
use super::common::reply;
use super::common::reply_error;
use super::common::tool_definition;
use super::common::wait_for_sent;

#[tokio::test]
async fn discovery_negotiates_the_modern_protocol_without_initialize() {
    let transport = modern_transport(|_| Ok(json!({})));
    let client = connect(Arc::clone(&transport)).await;
    assert_eq!(
        client.protocol_version().as_deref(),
        Some(LATEST_PROTOCOL_VERSION)
    );
    assert_eq!(client.protocol_era(), ProtocolEra::Modern);
    assert_eq!(
        client.instructions().as_deref(),
        Some("Use the tools wisely.")
    );
    assert_eq!(client.server_info().unwrap().name, "mock-server");
    assert!(client.server_capabilities().unwrap().tools.is_some());
    assert_eq!(transport.methods(), vec!["server/discover"]);
    assert_eq!(
        transport.protocol_version().as_deref(),
        Some(LATEST_PROTOCOL_VERSION)
    );
    client.close().await.unwrap();
    assert!(transport.is_closed());
    assert!(client.is_closed());
}

#[tokio::test]
async fn requests_carry_client_meta_and_results_require_result_type() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(json!({"tools": [tool_definition("echo")]})),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let tools = client
        .list_tools(None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(tools.tools.len(), 1);
    let request = transport.requests("tools/list").remove(0);
    insta::assert_json_snapshot!("modern_tools_list_request", request.params);
    let discover = transport.requests("server/discover").remove(0);
    let meta = &discover.params.unwrap()["_meta"];
    assert_eq!(
        meta["io.modelcontextprotocol/protocolVersion"],
        json!(LATEST_PROTOCOL_VERSION)
    );
    assert_eq!(
        meta["io.modelcontextprotocol/clientInfo"]["name"],
        json!("ferrin-mcp-client")
    );
    assert!(meta["io.modelcontextprotocol/clientCapabilities"].is_object());
}

#[tokio::test]
async fn missing_result_type_is_a_protocol_error() {
    let transport = MockTransport::new(discovery_capabilities(), |message| {
        let method = message.method().unwrap_or_default();
        Ok(match method {
            "server/discover" => vec![reply(message, discover_result())],
            "ping" => vec![reply(message, json!({}))],
            _ => Vec::new(),
        })
    });
    let client = connect(transport).await;
    let error = client.ping(RequestOptions::default()).await.unwrap_err();
    assert!(matches!(error, McpError::Protocol { ref message } if message.contains("resultType")));
}

#[tokio::test]
async fn input_required_results_are_answered_through_the_elicitation_handler() {
    let rounds = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = Arc::clone(&rounds);
    let transport = modern_transport(move |request| {
        if request.method != "tools/call" {
            return Err((-32601, "method not found".to_owned()));
        }
        let params = request.params.clone().unwrap();
        seen.lock().unwrap().push(Value::Object(params.clone()));
        if params.contains_key("inputResponses") {
            return Ok(call_tool_result("done"));
        }
        Ok(json!({
            "resultType": "input_required",
            "requestState": "state-1",
            "inputRequests": {
                "confirm": {
                    "method": "elicitation/create",
                    "params": {
                        "message": "Proceed?",
                        "requestedSchema": {"type": "object", "properties": {"ok": {"type": "boolean"}}}
                    }
                }
            }
        }))
    });
    let handler = elicitation_handler(|request| async move {
        assert_eq!(request.message, "Proceed?");
        Ok(ElicitResult::accept(
            json!({"ok": true}).as_object().cloned().unwrap(),
        ))
    });
    let client = McpClient::connect(config(Arc::clone(&transport)).elicitation_handler(handler))
        .await
        .unwrap();
    let result = client
        .call_tool(
            "echo",
            Some(json!({"city": "Berlin"}).as_object().cloned().unwrap()),
            RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.text(), "done");
    let rounds = rounds.lock().unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[1]["requestState"], json!("state-1"));
    assert_eq!(
        rounds[1]["inputResponses"],
        json!({"confirm": {"action": "accept", "content": {"ok": true}}})
    );
    assert_eq!(rounds[1]["name"], json!("echo"));
}

#[tokio::test]
async fn input_required_without_a_handler_fails() {
    let transport = modern_transport(|_| {
        Ok(json!({"resultType": "input_required", "inputRequests": {
            "a": {"method": "elicitation/create", "params": {"message": "?", "requestedSchema": {}}}
        }}))
    });
    let client = connect(transport).await;
    let error = client.ping(RequestOptions::default()).await.unwrap_err();
    assert!(matches!(error, McpError::Elicitation { .. }), "{error:?}");
}

#[tokio::test]
async fn capability_assertions_gate_method_families() {
    let transport = MockTransport::new(discovery_capabilities(), |message| {
        Ok(match message.method() {
            Some("server/discover") => {
                let mut result = discover_result();
                result["capabilities"] = json!({"tools": {}});
                vec![reply(message, result)]
            }
            Some(_) => vec![reply(message, complete(json!({"prompts": []})))],
            None => Vec::new(),
        })
    });
    let client = connect(Arc::clone(&transport)).await;
    let error = client
        .list_prompts(None, RequestOptions::default())
        .await
        .unwrap_err();
    assert!(
        matches!(error, McpError::UnsupportedCapability(ref what) if what.starts_with("prompts"))
    );
    assert!(transport.requests("prompts/list").is_empty());
    let error = client
        .list_resources(None, RequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::UnsupportedCapability(_)));
    let error = client
        .complete(
            ferrin_mcp::protocol::CompleteParams {
                reference: ferrin_mcp::protocol::CompletionReference::Prompt {
                    name: "p".to_owned(),
                },
                argument: ferrin_mcp::protocol::CompletionArgument {
                    name: "a".to_owned(),
                    value: String::new(),
                },
                context: None,
            },
            RequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::UnsupportedCapability(_)));
}

#[tokio::test]
async fn tool_calls_retry_transport_failures_but_not_json_rpc_errors() {
    let attempts = Arc::new(Mutex::new(0_u32));
    let counter = Arc::clone(&attempts);
    let transport = MockTransport::new(discovery_capabilities(), move |message| {
        let JsonRpcMessage::Request(request) = message else {
            return Ok(Vec::new());
        };
        match request.method.as_str() {
            "server/discover" => Ok(vec![reply(message, discover_result())]),
            "tools/call" => {
                let mut attempts = counter.lock().unwrap();
                *attempts += 1;
                if *attempts < 3 {
                    Err(McpError::http_status(
                        "unavailable",
                        http::StatusCode::SERVICE_UNAVAILABLE,
                        &url::Url::parse("https://mcp.example.com/mcp").unwrap(),
                        None,
                    ))
                } else if request.params.as_ref().unwrap()["name"] == json!("broken") {
                    Ok(vec![reply_error(message, -32602, "invalid params")])
                } else {
                    Ok(vec![reply(message, complete(call_tool_result("ok")))])
                }
            }
            _ => Ok(Vec::new()),
        }
    });
    let client = McpClient::connect(config(Arc::clone(&transport)).max_tool_call_retries(2))
        .await
        .unwrap();
    let result = client
        .call_tool("echo", None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(result.text(), "ok");
    assert_eq!(*attempts.lock().unwrap(), 3);
    let error = client
        .call_tool("broken", None, RequestOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(-32602));
    assert!(!error.is_retryable_tool_call());
    assert_eq!(*attempts.lock().unwrap(), 4);
}

#[tokio::test]
async fn tool_calls_without_retries_surface_the_first_failure() {
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![reply(message, discover_result())]),
            Some("tools/call") => Err(McpError::transport("connection reset")),
            _ => Ok(Vec::new()),
        });
    let client = connect(transport).await;
    let error = client
        .call_tool("echo", None, RequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::Transport(_)));
}

#[tokio::test]
async fn timeouts_cancel_the_request_on_the_server() {
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![reply(message, discover_result())]),
            _ => Ok(Vec::new()),
        });
    let client = connect(Arc::clone(&transport)).await;
    let error = client
        .ping(RequestOptions::with_timeout(Duration::from_millis(20)))
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::Timeout(_)));
    wait_for_sent(&transport, |sent| {
        sent.iter()
            .any(|message| message.method() == Some("notifications/cancelled"))
    })
    .await;
    let cancelled = transport.sent().into_iter().next_back().unwrap();
    let params = cancelled.params().unwrap();
    assert_eq!(params["reason"], json!("timeout"));
    assert!(params["requestId"].is_number());
}

#[tokio::test]
async fn server_ping_and_elicitation_requests_are_answered() {
    let transport = modern_transport(|_| Ok(json!({})));
    let handler = elicitation_handler(|_| async { Ok(ElicitResult::decline()) });
    let client = McpClient::connect(config(Arc::clone(&transport)).elicitation_handler(handler))
        .await
        .unwrap();
    transport.inject(TransportEvent::Message(JsonRpcMessage::request(
        "srv-1", "ping", None,
    )));
    transport.inject(TransportEvent::Message(JsonRpcMessage::request(
        "srv-2",
        "elicitation/create",
        json!({"message": "Name?", "requestedSchema": {"type": "object"}})
            .as_object()
            .cloned(),
    )));
    transport.inject(TransportEvent::Message(JsonRpcMessage::request(
        "srv-3",
        "sampling/createMessage",
        None,
    )));
    wait_for_sent(&transport, |sent| {
        sent.iter()
            .filter(|message| {
                matches!(
                    message,
                    JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_)
                )
            })
            .count()
            == 3
    })
    .await;
    let replies: Vec<Value> = transport
        .sent()
        .into_iter()
        .filter(|message| {
            matches!(
                message,
                JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_)
            )
        })
        .map(|message| message.to_json())
        .collect();
    assert_eq!(
        replies,
        vec![
            json!({"jsonrpc": "2.0", "id": "srv-1", "result": {}}),
            json!({"jsonrpc": "2.0", "id": "srv-2", "result": {"action": "decline"}}),
            json!({"jsonrpc": "2.0", "id": "srv-3", "error": {"code": -32601, "message": "method not found: sampling/createMessage"}}),
        ]
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn elicitation_without_handler_returns_method_not_found() {
    let transport = modern_transport(|_| Ok(json!({})));
    let client = connect(Arc::clone(&transport)).await;
    transport.inject(TransportEvent::Message(JsonRpcMessage::request(
        1,
        "elicitation/create",
        json!({"message": "?", "requestedSchema": {}})
            .as_object()
            .cloned(),
    )));
    wait_for_sent(&transport, |sent| {
        sent.iter()
            .any(|message| matches!(message, JsonRpcMessage::Error(_)))
    })
    .await;
    let error = transport
        .sent()
        .into_iter()
        .find_map(|message| match message {
            JsonRpcMessage::Error(error) => Some(error),
            _ => None,
        })
        .unwrap();
    assert_eq!(error.error.code, -32601);
    assert_eq!(
        error.error.message,
        "no elicitation handler registered on client"
    );
    drop(client);
}

#[tokio::test]
async fn uncaught_errors_reach_the_hook_and_closing_fails_pending_requests() {
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&errors);
    let transport =
        MockTransport::new(discovery_capabilities(), |message| match message.method() {
            Some("server/discover") => Ok(vec![reply(message, discover_result())]),
            _ => Ok(Vec::new()),
        });
    let client = McpClient::connect(config(Arc::clone(&transport)).on_uncaught_error(Arc::new(
        move |error| {
            sink.lock().unwrap().push(error.to_string());
        },
    )))
    .await
    .unwrap();
    transport.inject(TransportEvent::Error(McpError::protocol(
        "stray parse failure",
    )));
    transport.inject(TransportEvent::Message(JsonRpcMessage::response(
        999.into(),
        serde_json::Map::new(),
    )));
    let mut pending = tokio::task::JoinSet::new();
    {
        let client = client.clone();
        pending.spawn(async move {
            client
                .request("slow/method", None, RequestOptions::default())
                .await
        });
    }
    wait_for_sent(&transport, |sent| {
        sent.iter()
            .any(|message| message.method() == Some("slow/method"))
    })
    .await;
    transport.inject(TransportEvent::Closed);
    let outcome = pending.join_next().await.unwrap().unwrap();
    assert!(matches!(outcome, Err(McpError::Closed)), "{outcome:?}");
    assert!(client.is_closed());
    let errors = errors.lock().unwrap();
    assert!(
        errors
            .iter()
            .any(|error| error.contains("stray parse failure"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("unknown request id 999"))
    );
}
