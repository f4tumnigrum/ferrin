//! JSON-RPC framing.

use ferrin_mcp::McpError;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn request_round_trips_through_json() {
    let params = json!({"name": "get_weather", "arguments": {"city": "Berlin"}});
    let message = JsonRpcMessage::request(7, "tools/call", params.as_object().cloned());
    let text = message.to_json_string();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": params})
    );
    assert_eq!(JsonRpcMessage::parse(&text).unwrap(), message);
    assert_eq!(message.id(), Some(&RequestId::Number(7)));
    assert_eq!(message.method(), Some("tools/call"));
}

#[test]
fn classifies_every_message_shape() {
    let notification =
        JsonRpcMessage::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
    assert_eq!(
        notification,
        JsonRpcMessage::notification("notifications/initialized", None)
    );
    let response =
        JsonRpcMessage::parse(r#"{"jsonrpc":"2.0","id":"abc","result":{"ok":true}}"#).unwrap();
    assert_eq!(
        response,
        JsonRpcMessage::response(
            RequestId::from("abc"),
            json!({"ok": true}).as_object().cloned().unwrap()
        )
    );
    let error = JsonRpcMessage::parse(
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#,
    )
    .unwrap();
    assert_eq!(
        error,
        JsonRpcMessage::error(None, -32700, "parse error", None)
    );
    assert_eq!(error.id(), None);
}

#[test]
fn rejects_invalid_envelopes() {
    let wrong_version = JsonRpcMessage::parse(r#"{"jsonrpc":"1.0","id":1,"result":{}}"#);
    assert!(matches!(wrong_version, Err(McpError::Protocol { .. })));
    let no_shape = JsonRpcMessage::parse(r#"{"jsonrpc":"2.0","id":1}"#);
    assert!(matches!(no_shape, Err(McpError::Protocol { .. })));
    let not_json = JsonRpcMessage::parse("nope");
    assert!(matches!(not_json, Err(McpError::Protocol { .. })));
}

#[test]
fn parses_single_messages_and_batches() {
    let single =
        JsonRpcMessage::parse_one_or_many(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#).unwrap();
    assert_eq!(single.len(), 1);
    let batch = JsonRpcMessage::parse_one_or_many(
        r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","method":"notifications/progress","params":{"progress":1}}]"#,
    )
    .unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[1].method(), Some("notifications/progress"));
}

#[test]
fn request_ids_display_and_convert() {
    assert_eq!(RequestId::from(3).to_string(), "3");
    assert_eq!(RequestId::from("x-1").to_string(), "x-1");
    assert_eq!(
        serde_json::to_value(RequestId::from("x-1")).unwrap(),
        json!("x-1")
    );
}
