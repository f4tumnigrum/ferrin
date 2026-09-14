//! `x-mcp-header` bindings and header value encoding.

use ferrin_mcp::McpError;
use ferrin_mcp::transport::HeaderBinding;
use ferrin_mcp::transport::HeaderValueType;
use ferrin_mcp::transport::encode_header_value;
use ferrin_mcp::transport::header_bindings;
use ferrin_mcp::transport::tool_headers;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn plain_ascii_values_pass_through() {
    assert_eq!(encode_header_value("tenant-a"), "tenant-a");
    assert_eq!(encode_header_value("a b"), "a b");
}

#[test]
fn non_ascii_or_padded_values_are_base64_encoded() {
    assert_eq!(encode_header_value("café"), "=?base64?Y2Fmw6k=?=");
    assert_eq!(encode_header_value(" padded"), "=?base64?IHBhZGRlZA==?=");
    assert_eq!(
        encode_header_value("=?base64?x?="),
        "=?base64?PT9iYXNlNjQ/eD89?="
    );
}

#[test]
fn collects_bindings_from_reachable_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tenant": {"type": "string", "x-mcp-header": "X-Tenant"},
            "nested": {
                "type": "object",
                "properties": {
                    "retries": {"type": "integer", "x-mcp-header": "X-Retries"},
                    "dry": {"type": "boolean", "x-mcp-header": "X-Dry-Run"}
                }
            },
            "plain": {"type": "string"}
        }
    });
    let bindings = header_bindings(&schema).unwrap();
    assert_eq!(
        bindings,
        vec![
            HeaderBinding {
                header_name: "X-Tenant".to_owned(),
                path: vec!["tenant".to_owned()],
                value_type: HeaderValueType::String,
            },
            HeaderBinding {
                header_name: "X-Retries".to_owned(),
                path: vec!["nested".to_owned(), "retries".to_owned()],
                value_type: HeaderValueType::Integer,
            },
            HeaderBinding {
                header_name: "X-Dry-Run".to_owned(),
                path: vec!["nested".to_owned(), "dry".to_owned()],
                value_type: HeaderValueType::Boolean,
            },
        ]
    );
}

#[test]
fn rejects_invalid_bindings() {
    let duplicate = json!({"type": "object", "properties": {
        "a": {"type": "string", "x-mcp-header": "X-Dup"},
        "b": {"type": "string", "x-mcp-header": "x-dup"}
    }});
    assert!(matches!(
        header_bindings(&duplicate),
        Err(McpError::InvalidArgument { .. })
    ));
    let bad_type = json!({"type": "object", "properties": {
        "a": {"type": "object", "x-mcp-header": "X-Obj"}
    }});
    assert!(matches!(
        header_bindings(&bad_type),
        Err(McpError::InvalidArgument { .. })
    ));
    let bad_name = json!({"type": "object", "properties": {
        "a": {"type": "string", "x-mcp-header": "X Space"}
    }});
    assert!(matches!(
        header_bindings(&bad_name),
        Err(McpError::InvalidArgument { .. })
    ));
}

#[test]
fn tool_headers_encode_values_and_check_types() {
    let bindings = vec![
        HeaderBinding {
            header_name: "X-Tenant".to_owned(),
            path: vec!["tenant".to_owned()],
            value_type: HeaderValueType::String,
        },
        HeaderBinding {
            header_name: "X-Retries".to_owned(),
            path: vec!["nested".to_owned(), "retries".to_owned()],
            value_type: HeaderValueType::Integer,
        },
        HeaderBinding {
            header_name: "X-Missing".to_owned(),
            path: vec!["absent".to_owned()],
            value_type: HeaderValueType::String,
        },
    ];
    let arguments = json!({"tenant": "acme ü", "nested": {"retries": 3}});
    let headers = tool_headers(&bindings, arguments.as_object().unwrap()).unwrap();
    assert_eq!(
        headers,
        vec![
            (
                "Mcp-Param-X-Tenant".to_owned(),
                "=?base64?YWNtZSDDvA==?=".to_owned()
            ),
            ("Mcp-Param-X-Retries".to_owned(), "3".to_owned()),
        ]
    );
    let mismatch = json!({"tenant": 5});
    assert!(matches!(
        tool_headers(&bindings, mismatch.as_object().unwrap()),
        Err(McpError::InvalidArgument { .. })
    ));
}
