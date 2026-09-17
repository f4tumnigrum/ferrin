//! MCP Apps helpers.

use ferrin_mcp::McpError;
use ferrin_mcp::apps::MCP_APP_EXTENSION_NAME;
use ferrin_mcp::apps::MCP_APP_MIME_TYPE;
use ferrin_mcp::apps::McpAppResource;
use ferrin_mcp::apps::McpAppResourceCsp;
use ferrin_mcp::apps::McpAppResourceMeta;
use ferrin_mcp::apps::app_resource_from_read_result;
use ferrin_mcp::apps::app_resource_uris;
use ferrin_mcp::apps::app_tool_meta;
use ferrin_mcp::apps::detect_app_resource_drift;
use ferrin_mcp::apps::fingerprint_app_resource;
use ferrin_mcp::apps::is_app_tool;
use ferrin_mcp::apps::mcp_app_client_capabilities;
use ferrin_mcp::apps::split_app_tools;
use ferrin_mcp::protocol::McpTool;
use ferrin_mcp::protocol::ReadResourceResult;
use pretty_assertions::assert_eq;
use serde_json::json;

fn tool(name: &str, meta: serde_json::Value) -> McpTool {
    let mut value = json!({"name": name, "inputSchema": {"type": "object"}});
    if !meta.is_null() {
        value["_meta"] = meta;
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn app_metadata_is_read_from_ui_and_legacy_keys() {
    let modern = tool(
        "a",
        json!({"ui": {"resourceUri": "ui://w/a", "visibility": ["app"]}}),
    );
    let meta = app_tool_meta(&modern).unwrap().unwrap();
    assert_eq!(meta.resource_uri.as_deref(), Some("ui://w/a"));
    assert!(!meta.is_model_visible());
    assert!(meta.is_app_visible());
    let legacy = tool("b", json!({"ui/resourceUri": "ui://w/b"}));
    let meta = app_tool_meta(&legacy).unwrap().unwrap();
    assert_eq!(meta.resource_uri.as_deref(), Some("ui://w/b"));
    assert!(meta.is_model_visible() && !meta.is_app_visible());
    assert!(is_app_tool(&legacy));
    assert!(!is_app_tool(&tool("c", serde_json::Value::Null)));
    assert!(
        app_tool_meta(&tool("d", json!({"other": 1})))
            .unwrap()
            .is_none()
    );
}

#[test]
fn invalid_app_metadata_is_rejected() {
    let wrong_scheme = tool(
        "a",
        json!({"ui": {"resourceUri": "https://evil.example/w"}}),
    );
    assert!(matches!(
        app_tool_meta(&wrong_scheme),
        Err(McpError::InvalidArgument { .. })
    ));
    let not_object = tool("b", json!({"ui": "ui://w"}));
    assert_eq!(app_tool_meta(&not_object).unwrap(), None);
    let missing_uri = tool("c", json!({"ui": {"visibility": ["model"]}}));
    let meta = app_tool_meta(&missing_uri).unwrap().unwrap();
    assert_eq!(
        (meta.resource_uri, meta.visibility),
        (None, Some(vec!["model".into()]))
    );
}

#[test]
fn tools_are_split_by_audience() {
    let tools = vec![
        tool("plain", serde_json::Value::Null),
        tool("both", json!({"ui": {"resourceUri": "ui://w/both"}})),
        tool(
            "app_only",
            json!({"ui": {"resourceUri": "ui://w/app", "visibility": ["app"]}}),
        ),
        tool(
            "model_only",
            json!({"ui": {"resourceUri": "ui://w/both", "visibility": ["model"]}}),
        ),
    ];
    let split = split_app_tools(tools.clone()).unwrap();
    let names = |tools: &[McpTool]| {
        tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&split.model_tools),
        vec!["plain", "both", "model_only"]
    );
    assert_eq!(names(&split.app_tools), vec!["app_only"]);
    assert_eq!(
        app_resource_uris(&tools).unwrap(),
        vec!["ui://w/both", "ui://w/app"]
    );
}

#[test]
fn client_capabilities_declare_the_ui_extension() {
    let capabilities = mcp_app_client_capabilities();
    assert_eq!(
        serde_json::to_value(capabilities).unwrap(),
        json!({"extensions": {MCP_APP_EXTENSION_NAME: {"mimeTypes": [MCP_APP_MIME_TYPE]}}})
    );
}

fn read_result() -> ReadResourceResult {
    serde_json::from_value(json!({
        "contents": [
            {"uri": "ui://w/other", "mimeType": "text/plain", "text": "not it"},
            {
                "uri": "ui://w/a",
                "mimeType": "text/html;profile=mcp-app",
                "text": "<html>hi</html>",
                "_meta": {"ui": {
                    "prefersBorder": true,
                    "csp": {"connectDomains": ["https://api.example.com"]},
                    "permissions": {"camera": {}}
                }}
            }
        ]
    }))
    .unwrap()
}

#[test]
fn app_resources_are_extracted_and_fingerprinted() {
    let resource = app_resource_from_read_result("ui://w/a", &read_result()).unwrap();
    assert_eq!(
        resource,
        McpAppResource {
            uri: "ui://w/a".to_owned(),
            mime_type: "text/html;profile=mcp-app".to_owned(),
            html: "<html>hi</html>".to_owned(),
            meta: Some(McpAppResourceMeta {
                prefers_border: Some(true),
                csp: Some(McpAppResourceCsp {
                    connect_domains: Some(vec!["https://api.example.com".to_owned()]),
                    resource_domains: None,
                    frame_domains: None,
                    extra: serde_json::Map::new(),
                }),
                permissions: Some(json!({"camera": {}})),
                extra: serde_json::Map::new(),
            }),
        }
    );
    let fingerprint = fingerprint_app_resource(&resource);
    assert_eq!(fingerprint.len(), 43);
    assert!(!detect_app_resource_drift(&resource, &resource));
    let mut border_only = resource.clone();
    border_only.meta.as_mut().unwrap().prefers_border = Some(false);
    assert!(!detect_app_resource_drift(&border_only, &resource));
    let mut changed_html = resource.clone();
    changed_html.html.push_str("<!-- -->");
    assert!(detect_app_resource_drift(&changed_html, &resource));
    let mut changed_csp = resource.clone();
    changed_csp.meta.as_mut().unwrap().csp = None;
    assert!(detect_app_resource_drift(&changed_csp, &resource));
    let missing: ReadResourceResult = serde_json::from_value(json!({"contents": []})).unwrap();
    assert!(matches!(
        app_resource_from_read_result("ui://w/a", &missing),
        Err(McpError::Protocol { .. })
    ));
}

#[test]
fn visibility_only_metadata_and_legacy_uri_fallback_preserve_unknown_keys() {
    let app_only = tool(
        "app",
        json!({"ui":{"visibility":["app","unknown",3],"custom":true}}),
    );
    let legacy = tool(
        "legacy",
        json!({"ui":{"visibility":["model"]},"ui/resourceUri":"ui://legacy"}),
    );
    assert_eq!(
        serde_json::to_value(app_tool_meta(&app_only).unwrap().unwrap()).unwrap(),
        json!({"visibility":["app"],"custom":true}),
    );
    assert!(!is_app_tool(&app_only));
    assert_eq!(
        app_resource_uris(std::slice::from_ref(&legacy)).unwrap(),
        vec!["ui://legacy"]
    );
    let split = split_app_tools(vec![app_only.clone(), legacy.clone()]).unwrap();
    assert_eq!(
        (split.model_tools, split.app_tools),
        (vec![legacy], vec![app_only])
    );
}

#[test]
fn resource_matching_blob_decoding_and_tolerant_rendering_metadata_match_reference() {
    let result: ReadResourceResult = serde_json::from_value(json!({"contents":[
        {"uri":"ui://other","mimeType":MCP_APP_MIME_TYPE,"text":"wrong"},
        {"uri":"ui://chosen","mimeType":MCP_APP_MIME_TYPE,"blob":"PGI+aGk8L2I+","_meta":{"ui":{
            "prefersBorder":"invalid","permissions":[],"custom":7,
            "csp":{"connectDomains":["https://example.com",4],"resourceDomains":false,"customCsp":true}
        }}}
    ]})).unwrap();
    let resource = app_resource_from_read_result("ui://chosen", &result).unwrap();
    assert_eq!(
        (resource.html, serde_json::to_value(resource.meta).unwrap()),
        (
            "<b>hi</b>".into(),
            json!({"custom":7,"csp":{"connectDomains":["https://example.com"],"customCsp":true}})
        ),
    );
    assert!(app_resource_from_read_result("ui://absent", &result).is_err());
    let wrong_mime: ReadResourceResult = serde_json::from_value(json!({"contents":[
        {"uri":"ui://chosen","mimeType":"text/html; profile=mcp-app","text":"invalid"}
    ]}))
    .unwrap();
    assert!(app_resource_from_read_result("ui://chosen", &wrong_mime).is_err());
}
