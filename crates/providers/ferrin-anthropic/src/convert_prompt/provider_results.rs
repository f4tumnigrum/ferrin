//! Provider-executed tool calls and results echoed back in the prompt.
//!
//! The results were produced by this crate in camelCase (see `output.rs`);
//! this module converts them back to the wire format.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use serde_json::json;

use super::Converter;
use super::assistant::caller_block;

fn is_mcp(converter: &Converter<'_>, options: Option<&ferrin_spec::ProviderOptions>) -> bool {
    converter
        .options(options)
        .and_then(|o| o.get("type"))
        .and_then(JsonValue::as_str)
        == Some("mcp-tool-use")
}

fn input_object(input: &JsonValue) -> JsonObject {
    input.as_object().cloned().unwrap_or_default()
}

/// Converts a provider-executed tool call to `server_tool_use` or
/// `mcp_tool_use`.
pub(super) fn provider_tool_use(
    converter: &mut Converter<'_>,
    call: &ToolCallPart,
) -> Option<JsonValue> {
    let options = converter.options(call.provider_options.as_ref());
    if is_mcp(converter, call.provider_options.as_ref()) {
        let server_name = options
            .and_then(|o| o.get("serverName"))
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        return Some(json!({
            "type": "mcp_tool_use",
            "id": call.tool_call_id.as_str(),
            "name": call.tool_name.as_str(),
            "input": call.input,
            "server_name": server_name,
        }));
    }
    let provider_name = converter
        .mapping
        .to_provider_tool_name(call.tool_name.as_str())
        .to_owned();
    let (name, input) = match provider_name.as_str() {
        "code_execution" => {
            let mut input = input_object(&call.input);
            let kind = input
                .get("type")
                .and_then(JsonValue::as_str)
                .map(str::to_owned);
            match kind.as_deref() {
                Some(subtype @ ("bash_code_execution" | "text_editor_code_execution")) => {
                    input.remove("type");
                    (subtype.to_owned(), JsonValue::Object(input))
                }
                Some("programmatic-tool-call") => {
                    input.remove("type");
                    ("code_execution".to_owned(), JsonValue::Object(input))
                }
                _ => ("code_execution".to_owned(), JsonValue::Object(input)),
            }
        }
        "web_fetch" | "web_search" | "tool_search_tool_regex" | "tool_search_tool_bm25" => {
            (provider_name.clone(), call.input.clone())
        }
        "advisor" => ("advisor".to_owned(), json!({})),
        _ => {
            converter.warn_unsupported(format!("provider executed tool call: {}", call.tool_name));
            return None;
        }
    };
    let mut block = json!({
        "type": "server_tool_use",
        "id": call.tool_call_id.as_str(),
        "name": name,
        "input": input,
    });
    if let Some(caller) = caller_block(options)
        && let Some(object) = block.as_object_mut()
    {
        object.insert("caller".to_owned(), caller);
    }
    Some(block)
}

fn result_value(output: &ToolResultOutput) -> (JsonValue, bool) {
    match output {
        ToolResultOutput::Json { value, .. } => (value.clone(), false),
        ToolResultOutput::ErrorJson { value, .. } => (value.clone(), true),
        ToolResultOutput::Text { value, .. } => (JsonValue::from(value.clone()), false),
        ToolResultOutput::ErrorText { value, .. } => (JsonValue::from(value.clone()), true),
        _ => (JsonValue::Null, false),
    }
}

fn str_field(object: &JsonObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

fn error_code(object: &JsonObject) -> JsonValue {
    object
        .get("errorCode")
        .or_else(|| object.get("error_code"))
        .cloned()
        .unwrap_or(JsonValue::Null)
}

fn web_fetch_content(value: &JsonValue) -> JsonValue {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    if object.get("type").and_then(JsonValue::as_str) == Some("web_fetch_tool_result_error") {
        return json!({"type": "web_fetch_tool_result_error", "error_code": error_code(object)});
    }
    let content = object
        .get("content")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let source = content
        .get("source")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut document = json!({
        "type": content.get("type").cloned().unwrap_or_else(|| JsonValue::from("document")),
        "title": content.get("title").cloned().unwrap_or(JsonValue::Null),
        "source": {
            "type": source.get("type").cloned().unwrap_or(JsonValue::Null),
            "media_type": source.get("mediaType").or_else(|| source.get("media_type")).cloned().unwrap_or(JsonValue::Null),
            "data": source.get("data").cloned().unwrap_or(JsonValue::Null),
        },
    });
    if let Some(citations) = content.get("citations")
        && let Some(doc) = document.as_object_mut()
    {
        doc.insert("citations".to_owned(), citations.clone());
    }
    json!({
        "type": "web_fetch_result",
        "url": object.get("url").cloned().unwrap_or(JsonValue::Null),
        "retrieved_at": object.get("retrievedAt").or_else(|| object.get("retrieved_at")).cloned().unwrap_or(JsonValue::Null),
        "content": document,
    })
}

fn web_search_content(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(results) => JsonValue::Array(
            results
                .iter()
                .map(|result| {
                    let object = result.as_object().cloned().unwrap_or_default();
                    let mut mapped = json!({
                        "url": object.get("url").cloned().unwrap_or(JsonValue::Null),
                        "page_age": object.get("pageAge").or_else(|| object.get("page_age")).cloned().unwrap_or(JsonValue::Null),
                        "encrypted_content": object.get("encryptedContent").or_else(|| object.get("encrypted_content")).cloned().unwrap_or(JsonValue::Null),
                        "type": object.get("type").cloned().unwrap_or_else(|| JsonValue::from("web_search_result")),
                    });
                    if let Some(title) = object.get("title")
                        && let Some(entry) = mapped.as_object_mut()
                    {
                        entry.insert("title".to_owned(), title.clone());
                    }
                    mapped
                })
                .collect(),
        ),
        JsonValue::Object(object) => json!({
            "type": "web_search_tool_result_error",
            "error_code": error_code(object),
        }),
        other => other.clone(),
    }
}

fn tool_search_content(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(references) => json!({
            "type": "tool_search_tool_search_result",
            "tool_references": references
                .iter()
                .map(|reference| {
                    let object = reference.as_object().cloned().unwrap_or_default();
                    json!({
                        "type": "tool_reference",
                        "tool_name": object.get("toolName").or_else(|| object.get("tool_name")).cloned().unwrap_or(JsonValue::Null),
                    })
                })
                .collect::<Vec<_>>(),
        }),
        JsonValue::Object(object) => json!({
            "type": "tool_search_tool_result_error",
            "error_code": error_code(object),
        }),
        other => other.clone(),
    }
}

fn advisor_content(value: &JsonValue) -> JsonValue {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let kind = object.get("type").and_then(JsonValue::as_str).unwrap_or("");
    let mut content = match kind {
        "advisor_result" => json!({
            "type": "advisor_result",
            "text": object.get("text").cloned().unwrap_or(JsonValue::Null),
        }),
        "advisor_redacted_result" => json!({
            "type": "advisor_redacted_result",
            "encrypted_content": object.get("encryptedContent").or_else(|| object.get("encrypted_content")).cloned().unwrap_or(JsonValue::Null),
        }),
        _ => return json!({"type": "advisor_tool_result_error", "error_code": error_code(object)}),
    };
    if let Some(stop_reason) =
        str_field(object, "stopReason").or_else(|| str_field(object, "stop_reason"))
        && let Some(entry) = content.as_object_mut()
    {
        entry.insert("stop_reason".to_owned(), JsonValue::from(stop_reason));
    }
    content
}

/// Converts a provider-executed tool result to its wire block.
pub(super) fn provider_tool_result(
    converter: &mut Converter<'_>,
    result: &ToolResultPart,
) -> Option<JsonValue> {
    let (value, is_error) = result_value(&result.output);
    let tool_use_id = result.tool_call_id.as_str();
    if is_mcp(converter, result.provider_options.as_ref()) {
        return Some(json!({
            "type": "mcp_tool_result",
            "tool_use_id": tool_use_id,
            "is_error": is_error,
            "content": value,
        }));
    }
    let provider_name = converter
        .mapping
        .to_provider_tool_name(result.tool_name.as_str())
        .to_owned();
    let block = match provider_name.as_str() {
        "code_execution" => {
            let kind = value
                .get("type")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let block_type = if kind.starts_with("bash_code_execution") {
                "bash_code_execution_tool_result"
            } else if kind.starts_with("text_editor_code_execution") {
                "text_editor_code_execution_tool_result"
            } else {
                "code_execution_tool_result"
            };
            let content = if kind == "code_execution_tool_result_error" {
                json!({
                    "type": kind,
                    "error_code": value.as_object().map_or(JsonValue::Null, error_code),
                })
            } else {
                value.clone()
            };
            json!({"type": block_type, "tool_use_id": tool_use_id, "content": content})
        }
        "web_fetch" => json!({
            "type": "web_fetch_tool_result",
            "tool_use_id": tool_use_id,
            "content": web_fetch_content(&value),
        }),
        "web_search" => json!({
            "type": "web_search_tool_result",
            "tool_use_id": tool_use_id,
            "content": web_search_content(&value),
        }),
        "tool_search_tool_regex" | "tool_search_tool_bm25" => json!({
            "type": "tool_search_tool_result",
            "tool_use_id": tool_use_id,
            "content": tool_search_content(&value),
        }),
        "advisor" => json!({
            "type": "advisor_tool_result",
            "tool_use_id": tool_use_id,
            "content": advisor_content(&value),
        }),
        _ => {
            converter.warn_unsupported(format!(
                "provider executed tool result: {}",
                result.tool_name
            ));
            return None;
        }
    };
    Some(block)
}
