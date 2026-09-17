//! Mapping of Responses API output items to specification content.
//!
//! Shared by the non-streaming model, the streaming model (item added/done
//! events) and the batch service.
//!
//! Advanced tool behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::Usage;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::CustomKind;
use ferrin_spec::language_model::FinishReason;
use ferrin_spec::language_model::FinishReasonKind;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::ToolCall;
use serde_json::json;

use super::api_types::OutputItem;
use super::api_types::ResponsesUsage;
use crate::config::OpenAiConfig;
use crate::config::SharedConfig;

/// Builds provider metadata under the configured key.
#[must_use]
pub fn metadata(key: &str, object: JsonObject) -> ProviderMetadata {
    let mut map = ProviderMetadata::new();
    map.insert(key.to_owned(), object);
    map
}

/// Maps `usage` to the specification usage.
#[must_use]
pub fn map_usage(usage: &ResponsesUsage, raw: Option<JsonObject>) -> Usage {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let cached = usage
        .input_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);
    let cache_write = usage
        .input_tokens_details
        .as_ref()
        .and_then(|d| d.cache_write_tokens);
    let reasoning = usage
        .output_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens)
        .unwrap_or(0);
    let mut result = Usage::totals(input, output);
    result.input.no_cache = Some(
        input
            .saturating_sub(cached)
            .saturating_sub(cache_write.unwrap_or(0)),
    );
    result.input.cache_read = Some(cached);
    result.input.cache_write = cache_write;
    result.output.text = Some(output.saturating_sub(reasoning));
    result.output.reasoning = Some(reasoning);
    result.raw = raw;
    result
}

/// Maps the incomplete reason to a finish reason.
#[must_use]
pub fn map_finish_reason(reason: Option<&str>, has_function_call: bool) -> FinishReason {
    let unified = match reason {
        None => {
            if has_function_call {
                FinishReasonKind::ToolCalls
            } else {
                FinishReasonKind::Stop
            }
        }
        Some("max_output_tokens") => FinishReasonKind::Length,
        Some("content_filter") => FinishReasonKind::ContentFilter,
        Some(_) => {
            if has_function_call {
                FinishReasonKind::ToolCalls
            } else {
                FinishReasonKind::Other
            }
        }
    };
    FinishReason {
        unified,
        raw: reason.map(str::to_owned),
    }
}

/// Maps a `web_search_call.action` to the tool result value.
#[must_use]
pub fn map_web_search_output(action: Option<&JsonValue>) -> JsonValue {
    let Some(action) = action.filter(|a| !a.is_null()) else {
        return json!({});
    };
    match action.get("type").and_then(JsonValue::as_str) {
        Some("search") => {
            let mut inner = json!({"type": "search"});
            if let Some(object) = inner.as_object_mut() {
                if let Some(query) = action.get("query").filter(|q| !q.is_null()) {
                    object.insert("query".to_owned(), query.clone());
                }
                if let Some(queries) = action.get("queries").filter(|q| !q.is_null()) {
                    object.insert("queries".to_owned(), queries.clone());
                }
            }
            let mut result = json!({"action": inner});
            if let Some(sources) = action.get("sources").filter(|s| !s.is_null())
                && let Some(object) = result.as_object_mut()
            {
                object.insert("sources".to_owned(), sources.clone());
            }
            result
        }
        Some("open_page") => json!({"action": {"type": "openPage", "url": action.get("url")}}),
        Some("find_in_page") => json!({
            "action": {"type": "findInPage", "url": action.get("url"), "pattern": action.get("pattern")}
        }),
        _ => json!({}),
    }
}

/// Maps a `file_search_call` item to the tool result value.
#[must_use]
pub fn map_file_search_output(item: &OutputItem) -> JsonValue {
    let results = item
        .get("results")
        .and_then(JsonValue::as_array)
        .map(|results| {
            results
                .iter()
                .map(|result| {
                    json!({
                        "attributes": result.get("attributes"),
                        "fileId": result.get("file_id"),
                        "filename": result.get("filename"),
                        "score": result.get("score"),
                        "text": result.get("text"),
                    })
                })
                .collect::<Vec<_>>()
        });
    json!({
        "queries": item.get("queries").cloned().unwrap_or(JsonValue::Array(Vec::new())),
        "results": results,
    })
}

/// Maps a message annotation to a source part.
#[must_use]
pub fn annotation_source(annotation: &JsonValue, config: &OpenAiConfig) -> Option<Content> {
    let key = config.provider_options_key.as_str();
    let kind = annotation.get("type").and_then(JsonValue::as_str)?;
    let text = |field: &str| {
        annotation
            .get(field)
            .and_then(JsonValue::as_str)
            .map(str::to_owned)
    };
    let source = match kind {
        "url_citation" => Source::Url {
            id: config.generate_id(),
            url: text("url")?,
            title: text("title"),
            provider_metadata: None,
        },
        "file_citation" => {
            let file_id = text("file_id")?;
            let filename = text("filename").unwrap_or_else(|| file_id.clone());
            let mut meta = JsonObject::new();
            meta.insert("type".to_owned(), JsonValue::from(kind));
            meta.insert("fileId".to_owned(), JsonValue::from(file_id));
            if let Some(index) = annotation.get("index") {
                meta.insert("index".to_owned(), index.clone());
            }
            Source::Document {
                id: config.generate_id(),
                media_type: "text/plain".into(),
                title: filename.clone(),
                filename: Some(filename),
                provider_metadata: Some(metadata(key, meta)),
            }
        }
        "container_file_citation" => {
            let file_id = text("file_id")?;
            let filename = text("filename").unwrap_or_else(|| file_id.clone());
            let mut meta = JsonObject::new();
            meta.insert("type".to_owned(), JsonValue::from(kind));
            meta.insert("fileId".to_owned(), JsonValue::from(file_id));
            if let Some(container) = annotation.get("container_id") {
                meta.insert("containerId".to_owned(), container.clone());
            }
            Source::Document {
                id: config.generate_id(),
                media_type: "text/plain".into(),
                title: filename.clone(),
                filename: Some(filename),
                provider_metadata: Some(metadata(key, meta)),
            }
        }
        "file_path" => {
            let file_id = text("file_id")?;
            let mut meta = JsonObject::new();
            meta.insert("type".to_owned(), JsonValue::from(kind));
            meta.insert("fileId".to_owned(), JsonValue::from(file_id.as_str()));
            if let Some(index) = annotation.get("index") {
                meta.insert("index".to_owned(), index.clone());
            }
            Source::Document {
                id: config.generate_id(),
                media_type: "application/octet-stream".into(),
                title: file_id.clone(),
                filename: Some(file_id),
                provider_metadata: Some(metadata(key, meta)),
            }
        }
        _ => return None,
    };
    Some(Content::Source(source))
}

/// Maps output items to content, tracking client-side function calls.
#[derive(Debug)]
pub struct OutputMapper {
    /// Provider configuration.
    pub config: SharedConfig,
    /// Tool name mapping of the request.
    pub tool_name_mapping: ToolNameMapping,
    /// Custom name of the declared web search tool.
    pub web_search_tool_name: Option<String>,
    /// Approval request id → synthetic tool call id from the prompt.
    pub approval_tool_call_ids: HashMap<String, String>,
    /// Whether a client-executed tool call was produced.
    pub has_function_call: bool,
    /// Collected logprobs of text parts.
    pub logprobs: Vec<JsonValue>,
    pub(crate) function_names: HashSet<String>,
    pub(crate) hosted_shell: bool,
    pub(crate) hosted_search_ids: VecDeque<String>,
}

impl OutputMapper {
    /// Creates a mapper.
    #[must_use]
    pub fn new(
        config: SharedConfig,
        tool_name_mapping: ToolNameMapping,
        web_search_tool_name: Option<String>,
    ) -> Self {
        Self {
            config,
            tool_name_mapping,
            web_search_tool_name,
            approval_tool_call_ids: HashMap::new(),
            has_function_call: false,
            logprobs: Vec::new(),
            function_names: HashSet::new(),
            hosted_shell: false,
            hosted_search_ids: VecDeque::new(),
        }
    }

    pub(super) fn key(&self) -> &str {
        &self.config.provider_options_key
    }

    pub(super) fn custom_name(&self, provider_name: &str) -> String {
        self.tool_name_mapping
            .to_custom_tool_name(provider_name)
            .to_owned()
    }

    pub(super) fn item_metadata(&self, item: &OutputItem) -> ProviderMetadata {
        let mut meta = JsonObject::new();
        meta.insert(
            "itemId".to_owned(),
            item.id.clone().map_or(JsonValue::Null, JsonValue::from),
        );
        metadata(self.key(), meta)
    }

    /// Web search tool name in custom form.
    #[must_use]
    pub fn web_search_name(&self) -> String {
        self.custom_name(self.web_search_tool_name.as_deref().unwrap_or("web_search"))
    }

    /// Provider-executed tool call with `{}` input.
    #[must_use]
    pub fn provider_call(&self, id: &str, name: &str, input: &str) -> Content {
        let mut call = ToolCall::new(id, name, input);
        call.provider_executed = true;
        Content::ToolCall(call)
    }

    /// Provider-executed tool result.
    #[must_use]
    pub fn provider_result(&self, id: &str, name: &str, result: JsonValue) -> Content {
        Content::ToolResult(ProviderToolResult {
            tool_call_id: id.into(),
            tool_name: name.into(),
            result,
            is_error: false,
            preliminary: false,
            dynamic: false,
            provider_metadata: None,
        })
    }

    /// Maps the reasoning summaries of a `reasoning` item.
    #[must_use]
    pub fn reasoning_parts(&self, item: &OutputItem) -> Vec<Content> {
        let encrypted = item
            .get("encrypted_content")
            .cloned()
            .unwrap_or(JsonValue::Null);
        let meta = |_: ()| {
            let mut meta = JsonObject::new();
            meta.insert(
                "itemId".to_owned(),
                item.id.clone().map_or(JsonValue::Null, JsonValue::from),
            );
            meta.insert("reasoningEncryptedContent".to_owned(), encrypted.clone());
            Some(metadata(self.key(), meta))
        };
        let summaries: Vec<String> = item
            .get("summary")
            .and_then(JsonValue::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|s| s.get("text").and_then(JsonValue::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if summaries.is_empty() {
            return vec![Content::Reasoning {
                text: String::new(),
                provider_metadata: meta(()),
            }];
        }
        summaries
            .into_iter()
            .map(|text| Content::Reasoning {
                text,
                provider_metadata: meta(()),
            })
            .collect()
    }

    /// Maps a `message` item to text and source parts.
    #[must_use]
    pub fn message_parts(&mut self, item: &OutputItem, collect_logprobs: bool) -> Vec<Content> {
        let mut out = Vec::new();
        let Some(parts) = item.get("content").and_then(JsonValue::as_array) else {
            return out;
        };
        for part in parts {
            if part.get("type").and_then(JsonValue::as_str) != Some("output_text") {
                continue;
            }
            if collect_logprobs && let Some(logprobs) = part.get("logprobs") {
                self.logprobs.push(logprobs.clone());
            }
            let annotations = part
                .get("annotations")
                .and_then(JsonValue::as_array)
                .cloned()
                .unwrap_or_default();
            let mut meta = JsonObject::new();
            meta.insert(
                "itemId".to_owned(),
                item.id.clone().map_or(JsonValue::Null, JsonValue::from),
            );
            if let Some(phase) = item.get("phase").filter(|p| !p.is_null()) {
                meta.insert("phase".to_owned(), phase.clone());
            }
            if !annotations.is_empty() {
                meta.insert(
                    "annotations".to_owned(),
                    JsonValue::Array(annotations.clone()),
                );
            }
            out.push(Content::Text {
                text: part
                    .get("text")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                provider_metadata: Some(metadata(self.key(), meta)),
            });
            for annotation in &annotations {
                out.extend(annotation_source(annotation, &self.config));
            }
        }
        out
    }

    /// Maps a completed output item.
    #[must_use]
    pub fn map_item(&mut self, item: &OutputItem, collect_logprobs: bool) -> Vec<Content> {
        let id = item.id_str().to_owned();
        match item.kind.as_str() {
            "reasoning" => self.reasoning_parts(item),
            "message" => self.message_parts(item, collect_logprobs),
            "function_call" => {
                self.has_function_call = true;
                if let Some(calls) = super::parallel::expand(item, &self.function_names, self.key())
                {
                    return calls;
                }
                let mut meta = JsonObject::new();
                meta.insert("itemId".to_owned(), JsonValue::from(id));
                for (field, key) in [("async", "async"), ("namespace", "namespace")] {
                    if let Some(value) = item.get(field).filter(|v| !v.is_null()) {
                        meta.insert(key.to_owned(), value.clone());
                    }
                }
                if let Some(caller) = item.get("caller").filter(|v| !v.is_null()) {
                    let mapped =
                        if caller.get("type").and_then(JsonValue::as_str) == Some("program") {
                            json!({"type": "program", "callerId": caller.get("caller_id")})
                        } else {
                            caller.clone()
                        };
                    meta.insert("caller".to_owned(), mapped);
                }
                let mut call = ToolCall::new(
                    item.get_str("call_id").unwrap_or_default(),
                    self.custom_name(item.get_str("name").unwrap_or_default()),
                    item.get_str("arguments").unwrap_or("{}"),
                );
                call.provider_metadata = Some(metadata(self.key(), meta));
                vec![Content::ToolCall(call)]
            }
            "custom_tool_call" => {
                self.has_function_call = true;
                let input = item.get("input").cloned().unwrap_or(JsonValue::Null);
                let mut meta = JsonObject::new();
                meta.insert("itemId".to_owned(), JsonValue::from(id));
                if let Some(is_async) = item.get("async").filter(|v| !v.is_null()) {
                    meta.insert("async".to_owned(), is_async.clone());
                }
                let mut call = ToolCall::new(
                    item.get_str("call_id").unwrap_or_default(),
                    self.custom_name(item.get_str("name").unwrap_or_default()),
                    input.to_string(),
                );
                call.provider_metadata = Some(metadata(self.key(), meta));
                vec![Content::ToolCall(call)]
            }
            "web_search_call" => {
                let name = self.web_search_name();
                vec![
                    self.provider_call(&id, &name, "{}"),
                    self.provider_result(&id, &name, map_web_search_output(item.get("action"))),
                ]
            }
            "file_search_call" => {
                let name = self.custom_name("file_search");
                vec![
                    self.provider_call(&id, &name, "{}"),
                    self.provider_result(&id, &name, map_file_search_output(item)),
                ]
            }
            "code_interpreter_call" => {
                let name = self.custom_name("code_interpreter");
                let input =
                    json!({"code": item.get("code"), "containerId": item.get("container_id")});
                vec![
                    self.provider_call(&id, &name, &input.to_string()),
                    self.provider_result(&id, &name, json!({"outputs": item.get("outputs")})),
                ]
            }
            "image_generation_call" => {
                let name = self.custom_name("image_generation");
                vec![
                    self.provider_call(&id, &name, "{}"),
                    self.provider_result(&id, &name, json!({"result": item.get("result")})),
                ]
            }
            "mcp_call" => {
                let tool_call_id = item
                    .get_str("approval_request_id")
                    .and_then(|approval| self.approval_tool_call_ids.get(approval).cloned())
                    .unwrap_or_else(|| id.clone());
                let name = format!("mcp.{}", item.get_str("name").unwrap_or_default());
                let mut call = ToolCall::new(
                    tool_call_id.clone(),
                    name.clone(),
                    item.get_str("arguments").unwrap_or("{}"),
                );
                call.provider_executed = true;
                call.dynamic = true;
                let mut result = json!({
                    "type": "call",
                    "serverLabel": item.get("server_label"),
                    "name": item.get("name"),
                    "arguments": item.get("arguments"),
                });
                if let Some(object) = result.as_object_mut() {
                    if let Some(output) = item.get("output").filter(|v| !v.is_null()) {
                        object.insert("output".to_owned(), output.clone());
                    }
                    if let Some(error) = item.get("error").filter(|v| !v.is_null()) {
                        object.insert("error".to_owned(), error.clone());
                    }
                }
                vec![
                    Content::ToolCall(call),
                    Content::ToolResult(ProviderToolResult {
                        tool_call_id: tool_call_id.into(),
                        tool_name: name.into(),
                        result,
                        is_error: false,
                        preliminary: false,
                        dynamic: true,
                        provider_metadata: Some(self.item_metadata(item)),
                    }),
                ]
            }
            "mcp_approval_request" => {
                let approval_id = item
                    .get_str("approval_request_id")
                    .unwrap_or(&id)
                    .to_owned();
                let dummy = self.config.generate_id();
                let name = format!("mcp.{}", item.get_str("name").unwrap_or_default());
                let mut call = ToolCall::new(
                    dummy.clone(),
                    name,
                    item.get_str("arguments").unwrap_or("{}"),
                );
                call.provider_executed = true;
                call.dynamic = true;
                vec![
                    Content::ToolCall(call),
                    Content::ToolApprovalRequest {
                        approval_id: approval_id.into(),
                        tool_call_id: dummy.into(),
                        provider_metadata: None,
                    },
                ]
            }
            "apply_patch_call" => {
                self.has_function_call = true;
                let call_id = item.get_str("call_id").unwrap_or_default().to_owned();
                let input = json!({"callId": call_id, "operation": item.get("operation")});
                let mut call =
                    ToolCall::new(call_id, self.custom_name("apply_patch"), input.to_string());
                call.provider_metadata = Some(self.item_metadata(item));
                vec![Content::ToolCall(call)]
            }
            "local_shell_call" => {
                self.has_function_call = true;
                let action = item.get("action").cloned().unwrap_or(JsonValue::Null);
                let input = json!({"action": {
                    "type": "exec",
                    "command": action.get("command"),
                    "timeoutMs": action.get("timeout_ms"),
                    "user": action.get("user"),
                    "workingDirectory": action.get("working_directory"),
                    "env": action.get("env"),
                }});
                let mut call = ToolCall::new(
                    item.get_str("call_id").unwrap_or_default(),
                    self.custom_name("local_shell"),
                    input.to_string(),
                );
                call.provider_metadata = Some(self.item_metadata(item));
                vec![Content::ToolCall(call)]
            }
            "shell_call" => {
                self.has_function_call |= !self.hosted_shell;
                let action = item.get("action").cloned().unwrap_or(JsonValue::Null);
                let input = json!({"action": {
                    "commands": action.get("commands"),
                    "timeoutMs": action.get("timeout_ms"),
                    "maxOutputLength": action.get("max_output_length"),
                }});
                let mut call = ToolCall::new(
                    item.get_str("call_id").unwrap_or_default(),
                    self.custom_name("shell"),
                    input.to_string(),
                );
                call.provider_metadata = Some(self.item_metadata(item));
                call.provider_executed = self.hosted_shell;
                vec![Content::ToolCall(call)]
            }
            "program" | "program_output" | "tool_search_call" | "tool_search_output"
            | "shell_call_output" => self.advanced_item(item),
            "computer_call" => {
                self.has_function_call = true;
                let input = json!({
                    "action": item.get("action"),
                    "pendingSafetyChecks": item.get("pending_safety_checks"),
                });
                let mut call = ToolCall::new(
                    item.get_str("call_id").unwrap_or(&id),
                    self.custom_name("computer"),
                    input.to_string(),
                );
                call.provider_metadata = Some(self.item_metadata(item));
                vec![Content::ToolCall(call)]
            }
            "compaction" => {
                let mut meta = JsonObject::new();
                meta.insert("type".to_owned(), JsonValue::from("compaction"));
                meta.insert("itemId".to_owned(), JsonValue::from(id));
                meta.insert(
                    "encryptedContent".to_owned(),
                    item.get("encrypted_content")
                        .cloned()
                        .unwrap_or(JsonValue::Null),
                );
                CustomKind::parse("openai.compaction")
                    .ok()
                    .map(|kind| Content::Custom {
                        kind,
                        provider_metadata: Some(metadata(self.key(), meta)),
                    })
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Response-level provider metadata.
    #[must_use]
    pub fn response_metadata(
        &self,
        response_id: Option<&str>,
        service_tier: Option<&str>,
        reasoning_context: Option<&JsonValue>,
    ) -> ProviderMetadata {
        let mut meta = JsonObject::new();
        meta.insert(
            "responseId".to_owned(),
            response_id.map_or(JsonValue::Null, JsonValue::from),
        );
        if !self.logprobs.is_empty() {
            meta.insert(
                "logprobs".to_owned(),
                JsonValue::Array(self.logprobs.clone()),
            );
        }
        if let Some(tier) = service_tier {
            meta.insert("serviceTier".to_owned(), JsonValue::from(tier));
        }
        if let Some(context) = reasoning_context.filter(|c| !c.is_null()) {
            meta.insert("reasoningContext".to_owned(), context.clone());
        }
        metadata(self.key(), meta)
    }
}
