//! Tool bridging: schemas, metadata, execution and model output.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpError;
use ferrin_mcp::ToolSchemaPair;
use ferrin_mcp::ToolsOptions;
use ferrin_mcp::mcp_to_model_output;
use ferrin_schema::Schema;
use ferrin_spec::FileData;
use ferrin_spec::ToolCallId;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_tool::ModelOutputArgs;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolKind;
use ferrin_tool::execute_to_completion;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::common::call_tool_result;
use super::common::config;
use super::common::connect;
use super::common::modern_transport;
use super::common::tool_definition;

fn definitions() -> Value {
    json!({"tools": [
        tool_definition("echo"),
        {
            "name": "headered",
            "description": "Uses header bindings",
            "inputSchema": {
                "type": "object",
                "properties": {"tenant": {"type": "string", "x-mcp-header": "X-Tenant"}}
            },
            "_meta": {"ui": {"resourceUri": "ui://widgets/echo", "visibility": ["model", "app"]}}
        },
        {"name": "bare", "inputSchema": {"type": "object"}}
    ]})
}

fn run(tool: &ferrin_tool::Tool, input: Value) -> impl Future<Output = Result<Value, ToolError>> {
    let stream = tool
        .execute(input, ToolContext::new(ToolCallId::new("call-1")))
        .unwrap();
    async move { execute_to_completion(stream, |_| {}).await }
}

#[tokio::test]
async fn automatic_schemas_produce_dynamic_tools_with_metadata() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(definitions()),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let tools = client.tools(ToolsOptions::default()).await.unwrap();
    assert_eq!(
        tools.names().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["echo", "headered", "bare"]
    );
    let echo = tools.get("echo").unwrap();
    assert!(matches!(echo.kind(), ToolKind::Dynamic));
    assert_eq!(echo.title(), Some("echo title"));
    assert_eq!(
        echo.description().and_then(|d| d.as_static()),
        Some("echo description")
    );
    assert_eq!(
        echo.input_schema().json_schema(),
        &json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": false
        })
    );
    assert_eq!(
        Value::Object(echo.metadata().unwrap().clone()),
        json!({
            "clientName": "ferrin-mcp-client",
            "toolName": "echo",
            "title": "echo title",
            "annotations": {"readOnlyHint": true}
        })
    );
    let bare = tools.get("bare").unwrap();
    assert_eq!(
        bare.input_schema().json_schema(),
        &json!({"type": "object", "properties": {}, "additionalProperties": false})
    );
    let headered = tools.get("headered").unwrap();
    assert_eq!(
        headered.metadata().unwrap()["app"],
        json!({"resourceUri": "ui://widgets/echo", "visibility": ["model", "app"], "mimeType":"text/html;profile=mcp-app"})
    );
    assert!(!headered.metadata().unwrap().contains_key("meta"));
}

#[tokio::test]
async fn tools_paginate_and_apply_a_name_prefix() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => {
            let cursor = request
                .params
                .as_ref()
                .and_then(|params| params.get("cursor"))
                .and_then(Value::as_str);
            Ok(match cursor {
                None => json!({"tools": [tool_definition("first")], "nextCursor": "c2"}),
                Some("c2") => json!({"tools": [tool_definition("second")]}),
                Some(other) => panic!("unexpected cursor {other}"),
            })
        }
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let tools = client
        .tools(ToolsOptions::default().name_prefix("mcp_"))
        .await
        .unwrap();
    assert_eq!(
        tools.names().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["mcp_first", "mcp_second"]
    );
    assert_eq!(transport.requests("tools/list").len(), 2);
}

#[tokio::test]
async fn executing_a_tool_calls_the_server_and_maps_errors() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(definitions()),
        "tools/call" => {
            let params = request.params.as_ref().unwrap();
            match params["name"].as_str().unwrap() {
                "echo" => Ok(json!({
                    "content": [{"type": "text", "text": format!("echo {}", params["arguments"]["city"])}],
                    "structuredContent": {"city": params["arguments"]["city"]}
                })),
                "bare" => {
                    Ok(json!({"content": [{"type": "text", "text": "boom"}], "isError": true}))
                }
                other => Err((-32602, format!("unknown tool {other}"))),
            }
        }
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let tools = client.tools(ToolsOptions::default()).await.unwrap();
    let output = run(tools.get("echo").unwrap(), json!({"city": "Berlin"}))
        .await
        .unwrap();
    assert_eq!(output["content"][0]["text"], json!("echo \"Berlin\""));
    assert_eq!(output["structuredContent"], json!({"city": "Berlin"}));
    assert_eq!(output["isError"], json!(false));
    assert_eq!(output["resultType"], json!("complete"));
    let call = transport.requests("tools/call").remove(0).params.unwrap();
    assert_eq!(call["name"], json!("echo"));
    assert_eq!(call["arguments"], json!({"city": "Berlin"}));
    let output = run(tools.get("bare").unwrap(), json!({})).await.unwrap();
    assert_eq!(
        output,
        json!({
            "content":[{"type":"text","text":"boom"}],
            "isError":true,
            "resultType":"complete"
        })
    );
    let invalid = tools
        .get("echo")
        .unwrap()
        .validate_input(&ferrin_spec::ToolName::from("echo"), json!({"city": 1}));
    assert!(invalid.is_err());
}

#[tokio::test]
async fn explicit_schemas_filter_tools_and_validate_structured_output() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(definitions()),
        "tools/call" => {
            let name = request.params.as_ref().unwrap()["name"]
                .as_str()
                .unwrap()
                .to_owned();
            Ok(match name.as_str() {
                "echo" => json!({"content": [{"type": "text", "text": "{\"temperature\": 21}"}]}),
                "bare" => json!({"content": [], "structuredContent": {"temperature": "hot"}}),
                _ => json!({"content": []}),
            })
        }
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let output_schema = Schema::from_json_schema(json!({
        "type": "object",
        "properties": {"temperature": {"type": "integer"}},
        "required": ["temperature"]
    }));
    let schemas = HashMap::from([
        (
            "echo".to_owned(),
            ToolSchemaPair {
                input: Schema::from_json_schema(
                    json!({"type": "object", "properties": {"city": {"type": "string"}}}),
                ),
                output: Some(output_schema.clone()),
            },
        ),
        (
            "bare".to_owned(),
            ToolSchemaPair {
                input: Schema::from_json_schema(json!({"type": "object"})),
                output: Some(output_schema),
            },
        ),
    ]);
    let tools = client.tools(ToolsOptions::explicit(schemas)).await.unwrap();
    assert_eq!(
        tools.names().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["echo", "bare"]
    );
    let echo = tools.get("echo").unwrap();
    assert!(matches!(echo.kind(), ToolKind::Function));
    assert!(echo.output_schema().is_some());
    let output = run(echo, json!({"city": "Berlin"})).await.unwrap();
    assert_eq!(output, json!({"temperature": 21}));
    let error = run(tools.get("bare").unwrap(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ToolError::Message { message, .. } if message.contains("output schema")),
        "{error:?}"
    );
}

#[tokio::test]
async fn invalid_header_bindings_drop_the_tool_and_report() {
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&errors);
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(json!({"tools": [
            tool_definition("fine"),
            {"name": "bad", "inputSchema": {"type": "object", "properties": {
                "a": {"type": "object", "x-mcp-header": "X-A"}
            }}}
        ]})),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = McpClient::connect(config(transport).on_uncaught_error(Arc::new(move |error| {
        sink.lock().unwrap().push(error.to_string());
    })))
    .await
    .unwrap();
    let tools = client.tools(ToolsOptions::default()).await.unwrap();
    assert_eq!(
        tools.names().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["fine"]
    );
    let errors = errors.lock().unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("tool bad dropped"), "{errors:?}");
}

#[tokio::test]
async fn duplicate_tool_names_are_rejected() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/list" => Ok(json!({"tools": [tool_definition("dup"), tool_definition("dup")]})),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(transport).await;
    let error = client.tools(ToolsOptions::default()).await.unwrap_err();
    assert!(matches!(error, McpError::InvalidArgument { ref message } if message.contains("dup")));
}

#[test]
fn model_output_converts_content_arrays_and_passes_json_through() {
    let id = ToolCallId::new("call-1");
    let input = json!({});
    let output = call_tool_result("hello");
    let mut output = output;
    output["content"] = json!([
        {"type": "text", "text": "hello"},
        {"type": "image", "data": "AQID", "mimeType": "image/png"},
        {"type": "audio", "data": "AQID"},
        {"type": "resource_link", "uri": "file:///a", "name": "a"},
        {"type": "image", "data": "not base64!"}
    ]);
    let converted = mcp_to_model_output(ModelOutputArgs {
        tool_call_id: &id,
        input: &input,
        output: &output,
    });
    let ToolResultOutput::Content { value } = converted else {
        panic!("expected content output");
    };
    assert_eq!(value.len(), 5);
    assert!(matches!(&value[0], ToolResultContentPart::Text { text, .. } if text == "hello"));
    assert!(matches!(
        &value[1],
        ToolResultContentPart::File { data: FileData::Bytes { data }, media_type, .. }
            if data.as_ref() == [1, 2, 3] && media_type.as_str() == "image/png"
    ));
    assert!(matches!(
        &value[2],
        ToolResultContentPart::Text { text, .. } if serde_json::from_str::<Value>(text).unwrap() == json!({"type":"audio","data":"AQID"})
    ));
    assert!(matches!(
        &value[3],
        ToolResultContentPart::Text { text, .. } if text.contains("resource_link")
    ));
    assert!(matches!(&value[4], ToolResultContentPart::Text { .. }));
    let structured = json!({"temperature": 21});
    let converted = mcp_to_model_output(ModelOutputArgs {
        tool_call_id: &id,
        input: &input,
        output: &structured,
    });
    assert!(matches!(converted, ToolResultOutput::Json { value, .. } if value == structured));
}

#[tokio::test]
async fn annotations_title_metadata_and_error_results_match_reference() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "tools/call" => Ok(json!({"content":[{"type":"text","text":"refused"}],"isError":true})),
        _ => Err((-32601, "method not found".into())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let definition = serde_json::from_value(json!({
        "name":"guarded","inputSchema":{"type":"object"},
        "annotations":{"title":"annotation title","readOnlyHint":false,"unknown":true},
        "_meta":{"ui":{"visibility":["app"]}}
    }))
    .unwrap();
    let tools = client
        .tools_from_definitions(
            vec![definition],
            &ToolsOptions::explicit(HashMap::from([(
                "guarded".into(),
                ToolSchemaPair {
                    input: Schema::empty_object(),
                    output: Some(Schema::from_json_schema(json!({"type":"integer"}))),
                },
            )])),
        )
        .unwrap();
    let guarded = tools.get("guarded").unwrap();
    assert_eq!(
        (
            guarded.title(),
            Value::Object(guarded.metadata().unwrap().clone())
        ),
        (
            Some("annotation title"),
            json!({
                "clientName":"ferrin-mcp-client","toolName":"guarded","title":"annotation title",
                "annotations":{"title":"annotation title","readOnlyHint":false}
            })
        )
    );
    assert_eq!(
        run(guarded, json!({})).await.unwrap(),
        json!({
            "content":[{"type":"text","text":"refused"}],"isError":true,"resultType":"complete"
        })
    );
}
