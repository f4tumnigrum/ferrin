//! Result blocks of provider-executed tools (and MCP results).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_spec::language_model::Source;
use serde_json::json;

use super::CitationDocument;
use super::OutputMapper;
use super::anthropic_metadata;
use super::caller_metadata;
use crate::api_types::ContentBlock;

impl OutputMapper {
    /// Maps the result blocks of provider-executed tools (and MCP results);
    /// returns `None` for other block types.
    #[allow(clippy::too_many_lines, reason = "one arm per result block type")]
    pub fn map_result_block(&mut self, block: &ContentBlock) -> Option<Vec<Content>> {
        let content = match block {
            ContentBlock::McpToolResult {
                tool_use_id,
                is_error,
                content,
            } => {
                let (name, metadata) = self
                    .mcp_tool_calls
                    .get(tool_use_id)
                    .cloned()
                    .unwrap_or_else(|| (ToolName::new("mcp"), None));
                let mut result = provider_result(tool_use_id, name, content.clone());
                result.is_error = *is_error;
                result.dynamic = true;
                result.provider_metadata = metadata;
                vec![Content::ToolResult(result)]
            }
            ContentBlock::WebFetchToolResult {
                tool_use_id,
                content,
                caller,
            } => {
                let name = self.custom_name("web_fetch");
                let kind = content.get("type").and_then(JsonValue::as_str);
                let mut result = match kind {
                    Some("web_fetch_result") => {
                        let page = content.get("content").cloned().unwrap_or(JsonValue::Null);
                        let source = page.get("source").cloned().unwrap_or(JsonValue::Null);
                        let url = content.get("url").cloned().unwrap_or(JsonValue::Null);
                        self.citation_documents.push(CitationDocument {
                            title: page
                                .get("title")
                                .and_then(JsonValue::as_str)
                                .or_else(|| url.as_str())
                                .unwrap_or_default()
                                .to_owned(),
                            filename: None,
                            media_type: source
                                .get("media_type")
                                .and_then(JsonValue::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        });
                        provider_result(
                            tool_use_id,
                            name,
                            json!({
                                "type": "web_fetch_result",
                                "url": url,
                                "retrievedAt": content.get("retrieved_at").cloned().unwrap_or(JsonValue::Null),
                                "content": {
                                    "type": page.get("type").cloned().unwrap_or(JsonValue::Null),
                                    "title": page.get("title").cloned().unwrap_or(JsonValue::Null),
                                    "citations": page.get("citations").cloned().unwrap_or(JsonValue::Null),
                                    "source": {
                                        "type": source.get("type").cloned().unwrap_or(JsonValue::Null),
                                        "mediaType": source.get("media_type").cloned().unwrap_or(JsonValue::Null),
                                        "data": source.get("data").cloned().unwrap_or(JsonValue::Null),
                                    },
                                },
                            }),
                        )
                    }
                    Some("web_fetch_tool_result_error") => {
                        error_result(tool_use_id, name, "web_fetch_tool_result_error", content)
                    }
                    _ => return Some(Vec::new()),
                };
                result.provider_metadata = caller_metadata(caller.as_ref());
                vec![Content::ToolResult(result)]
            }
            ContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
                caller,
            } => {
                let name = self.custom_name("web_search");
                match content.as_array() {
                    Some(results) => {
                        let mapped: Vec<JsonValue> =
                            results.iter().map(web_search_result).collect();
                        let mut result =
                            provider_result(tool_use_id, name, JsonValue::Array(mapped));
                        result.provider_metadata = caller_metadata(caller.as_ref());
                        let mut content = vec![Content::ToolResult(result)];
                        for entry in results {
                            let Some(url) = entry.get("url").and_then(JsonValue::as_str) else {
                                continue;
                            };
                            let mut meta = JsonObject::new();
                            meta.insert(
                                "pageAge".to_owned(),
                                entry.get("page_age").cloned().unwrap_or(JsonValue::Null),
                            );
                            content.push(Content::Source(Source::Url {
                                id: self.config.generate_id(),
                                url: url.to_owned(),
                                title: entry
                                    .get("title")
                                    .and_then(JsonValue::as_str)
                                    .map(str::to_owned),
                                provider_metadata: Some(anthropic_metadata(meta)),
                            }));
                        }
                        content
                    }
                    None => {
                        let mut result = error_result(
                            tool_use_id,
                            name,
                            "web_search_tool_result_error",
                            content,
                        );
                        result.provider_metadata = caller_metadata(caller.as_ref());
                        vec![Content::ToolResult(result)]
                    }
                }
            }
            ContentBlock::CodeExecutionToolResult {
                tool_use_id,
                content,
            } => {
                let name = self.custom_name("code_execution");
                let kind = content.get("type").and_then(JsonValue::as_str);
                let result = match kind {
                    Some("code_execution_result") => provider_result(
                        tool_use_id,
                        name,
                        json!({
                            "type": "code_execution_result",
                            "stdout": content.get("stdout").cloned().unwrap_or(JsonValue::Null),
                            "stderr": content.get("stderr").cloned().unwrap_or(JsonValue::Null),
                            "return_code": content.get("return_code").cloned().unwrap_or(JsonValue::Null),
                            "content": content.get("content").cloned().unwrap_or_else(|| json!([])),
                        }),
                    ),
                    Some("encrypted_code_execution_result") => provider_result(
                        tool_use_id,
                        name,
                        json!({
                            "type": "encrypted_code_execution_result",
                            "encrypted_stdout": content.get("encrypted_stdout").cloned().unwrap_or(JsonValue::Null),
                            "stderr": content.get("stderr").cloned().unwrap_or(JsonValue::Null),
                            "return_code": content.get("return_code").cloned().unwrap_or(JsonValue::Null),
                            "content": content.get("content").cloned().unwrap_or_else(|| json!([])),
                        }),
                    ),
                    Some("code_execution_tool_result_error") => error_result(
                        tool_use_id,
                        name,
                        "code_execution_tool_result_error",
                        content,
                    ),
                    _ => return Some(Vec::new()),
                };
                vec![Content::ToolResult(result)]
            }
            ContentBlock::BashCodeExecutionToolResult {
                tool_use_id,
                content,
            }
            | ContentBlock::TextEditorCodeExecutionToolResult {
                tool_use_id,
                content,
            } => vec![Content::ToolResult(provider_result(
                tool_use_id,
                self.custom_name("code_execution"),
                content.clone(),
            ))],
            ContentBlock::ToolSearchToolResult {
                tool_use_id,
                content,
            } => {
                let provider_name = match self.tool_search_calls.get(tool_use_id) {
                    Some(name) => name.clone(),
                    None => {
                        if self.mapping.to_custom_tool_name("tool_search_tool_bm25")
                            != "tool_search_tool_bm25"
                        {
                            "tool_search_tool_bm25".to_owned()
                        } else {
                            "tool_search_tool_regex".to_owned()
                        }
                    }
                };
                let name = self.custom_name(&provider_name);
                let result = if content.get("type").and_then(JsonValue::as_str)
                    == Some("tool_search_tool_search_result")
                {
                    let references: Vec<JsonValue> = content
                        .get("tool_references")
                        .and_then(JsonValue::as_array)
                        .map(|references| {
                            references
                                .iter()
                                .map(|reference| {
                                    json!({
                                        "type": reference.get("type").cloned().unwrap_or(JsonValue::Null),
                                        "toolName": reference.get("tool_name").cloned().unwrap_or(JsonValue::Null),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    provider_result(tool_use_id, name, JsonValue::Array(references))
                } else {
                    error_result(tool_use_id, name, "tool_search_tool_result_error", content)
                };
                vec![Content::ToolResult(result)]
            }
            ContentBlock::AdvisorToolResult {
                tool_use_id,
                content,
            } => {
                let name = self.custom_name("advisor");
                let kind = content.get("type").and_then(JsonValue::as_str);
                let result = match kind {
                    Some("advisor_result") => {
                        let mut value = JsonObject::new();
                        value.insert("type".to_owned(), JsonValue::from("advisor_result"));
                        value.insert(
                            "text".to_owned(),
                            content.get("text").cloned().unwrap_or(JsonValue::Null),
                        );
                        if let Some(stop) = content.get("stop_reason").filter(|v| !v.is_null()) {
                            value.insert("stopReason".to_owned(), stop.clone());
                        }
                        provider_result(tool_use_id, name, JsonValue::Object(value))
                    }
                    Some("advisor_redacted_result") => {
                        let mut value = JsonObject::new();
                        value.insert(
                            "type".to_owned(),
                            JsonValue::from("advisor_redacted_result"),
                        );
                        value.insert(
                            "encryptedContent".to_owned(),
                            content
                                .get("encrypted_content")
                                .cloned()
                                .unwrap_or(JsonValue::Null),
                        );
                        if let Some(stop) = content.get("stop_reason").filter(|v| !v.is_null()) {
                            value.insert("stopReason".to_owned(), stop.clone());
                        }
                        provider_result(tool_use_id, name, JsonValue::Object(value))
                    }
                    _ => error_result(tool_use_id, name, "advisor_tool_result_error", content),
                };
                vec![Content::ToolResult(result)]
            }
            _ => return None,
        };
        Some(content)
    }
}

fn web_search_result(entry: &JsonValue) -> JsonValue {
    let mut value = JsonObject::new();
    value.insert(
        "url".to_owned(),
        entry.get("url").cloned().unwrap_or(JsonValue::Null),
    );
    if let Some(title) = entry.get("title").filter(|title| !title.is_null()) {
        value.insert("title".to_owned(), title.clone());
    }
    value.insert(
        "pageAge".to_owned(),
        entry.get("page_age").cloned().unwrap_or(JsonValue::Null),
    );
    value.insert(
        "encryptedContent".to_owned(),
        entry
            .get("encrypted_content")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    value.insert(
        "type".to_owned(),
        entry.get("type").cloned().unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(value)
}

fn provider_result(id: &str, name: ToolName, result: JsonValue) -> ProviderToolResult {
    ProviderToolResult {
        tool_call_id: ToolCallId::new(id),
        tool_name: name,
        result,
        is_error: false,
        preliminary: false,
        dynamic: false,
        provider_metadata: None,
    }
}

fn error_result(id: &str, name: ToolName, kind: &str, content: &JsonValue) -> ProviderToolResult {
    let mut result = provider_result(
        id,
        name,
        json!({
            "type": kind,
            "errorCode": content.get("error_code").cloned().unwrap_or(JsonValue::Null),
        }),
    );
    result.is_error = true;
    result
}
