//! Resources, prompts and completion over a mock transport.

use std::collections::BTreeMap;
use std::sync::Arc;

use ferrin_mcp::RequestOptions;
use ferrin_mcp::protocol::CompleteParams;
use ferrin_mcp::protocol::CompletionArgument;
use ferrin_mcp::protocol::CompletionContext;
use ferrin_mcp::protocol::CompletionReference;
use ferrin_mcp::protocol::Content;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::connect;
use super::common::modern_transport;

#[tokio::test]
async fn resources_are_listed_and_read() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "resources/list" => Ok(json!({
            "resources": [{"uri": "file:///a.txt", "name": "a", "mimeType": "text/plain", "size": 3}],
            "nextCursor": "page-2"
        })),
        "resources/templates/list" => Ok(json!({
            "resourceTemplates": [{"uriTemplate": "file:///{path}", "name": "files"}]
        })),
        "resources/read" => Ok(json!({
            "contents": [
                {"uri": "file:///a.txt", "mimeType": "text/plain", "text": "abc"},
                {"uri": "file:///b.bin", "mimeType": "application/octet-stream", "blob": "AQID"}
            ]
        })),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let list = client
        .list_resources(Some("page-1"), RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(list.resources[0].uri, "file:///a.txt");
    assert_eq!(list.resources[0].size, Some(3));
    assert_eq!(list.next_cursor.as_deref(), Some("page-2"));
    assert_eq!(
        transport
            .requests("resources/list")
            .remove(0)
            .params
            .unwrap()["cursor"],
        json!("page-1")
    );
    let templates = client
        .list_resource_templates(None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(
        templates.resource_templates[0].uri_template,
        "file:///{path}"
    );
    let read = client
        .read_resource("file:///a.txt", RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(read.contents.len(), 2);
    assert_eq!(read.contents[0].text.as_deref(), Some("abc"));
    assert_eq!(read.contents[1].blob.as_deref(), Some("AQID"));
    assert_eq!(
        transport
            .requests("resources/read")
            .remove(0)
            .params
            .unwrap()["uri"],
        json!("file:///a.txt")
    );
}

#[tokio::test]
async fn prompts_are_listed_and_fetched_with_arguments() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "prompts/list" => Ok(json!({
            "prompts": [{"name": "summarize", "arguments": [{"name": "text", "required": true}]}]
        })),
        "prompts/get" => Ok(json!({
            "description": "Summary prompt",
            "messages": [
                {"role": "user", "content": {"type": "text", "text": "Summarize: hello"}},
                {"role": "user", "content": {"type": "image", "data": "AQID", "mimeType": "image/png"}}
            ]
        })),
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let prompts = client
        .list_prompts(None, RequestOptions::default())
        .await
        .unwrap();
    assert_eq!(prompts.prompts[0].name, "summarize");
    assert_eq!(
        prompts.prompts[0].arguments.as_ref().unwrap()[0].required,
        Some(true)
    );
    let prompt = client
        .get_prompt(
            "summarize",
            Some(BTreeMap::from([("text".to_owned(), "hello".to_owned())])),
            RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(prompt.description.as_deref(), Some("Summary prompt"));
    assert_eq!(prompt.messages.len(), 2);
    assert!(
        matches!(&prompt.messages[0].content, Content::Text { text, .. } if text == "Summarize: hello")
    );
    assert!(
        matches!(&prompt.messages[1].content, Content::Image { mime_type, .. } if mime_type == "image/png")
    );
    let params = transport.requests("prompts/get").remove(0).params.unwrap();
    assert_eq!(params["name"], json!("summarize"));
    assert_eq!(params["arguments"], json!({"text": "hello"}));
}

#[tokio::test]
async fn completion_requests_serialize_the_reference() {
    let transport = modern_transport(|request| match request.method.as_str() {
        "completion/complete" => {
            Ok(json!({"completion": {"values": ["Berlin", "Bern"], "hasMore": false}}))
        }
        _ => Err((-32601, "method not found".to_owned())),
    });
    let client = connect(Arc::clone(&transport)).await;
    let result = client
        .complete(
            CompleteParams {
                reference: CompletionReference::Resource {
                    uri: "file:///{path}".to_owned(),
                },
                argument: CompletionArgument {
                    name: "path".to_owned(),
                    value: "Be".to_owned(),
                },
                context: Some(CompletionContext {
                    arguments: BTreeMap::from([("country".to_owned(), "DE".to_owned())]),
                }),
            },
            RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.completion.values, vec!["Berlin", "Bern"]);
    assert_eq!(result.completion.has_more, Some(false));
    let mut params = transport
        .requests("completion/complete")
        .remove(0)
        .params
        .unwrap();
    params.remove("_meta");
    assert_eq!(
        serde_json::Value::Object(params),
        json!({
            "ref": {"type": "ref/resource", "uri": "file:///{path}"},
            "argument": {"name": "path", "value": "Be"},
            "context": {"arguments": {"country": "DE"}}
        })
    );
}
