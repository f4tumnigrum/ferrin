//! Mapping of response content blocks to specification content and of the
//! response envelope to provider metadata.

mod metadata;
mod results;

use std::collections::HashMap;

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolName;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::CustomKind;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::Source;
use ferrin_spec::language_model::ToolCall;
use ferrin_spec::language_model::ToolDefinition;
use ferrin_spec::language_model::prompt::UserPromptPart;

use crate::api_types::Caller;
use crate::api_types::ContentBlock;
use crate::config::AnthropicConfig;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::SharedConfig;
use crate::options::FilePartOptions;
use crate::options::part_options;
use crate::options::read_options;

pub use self::metadata::MessageMetadata;
pub use self::metadata::container_metadata;
pub use self::metadata::context_management_metadata;

/// Whether the tools contain a 20260209 web tool but no code execution
/// tool; `code_execution` calls are then reported as dynamic.
#[must_use]
pub fn web_tool_20260209_without_code_execution(tools: &[ToolDefinition]) -> bool {
    let mut has_web_tool = false;
    for tool in tools {
        let ToolDefinition::Provider { id, .. } = tool else {
            continue;
        };
        match id.as_str() {
            "anthropic.web_fetch_20260209" | "anthropic.web_search_20260209" => has_web_tool = true,
            "anthropic.code_execution_20250522"
            | "anthropic.code_execution_20250825"
            | "anthropic.code_execution_20260120" => return false,
            _ => {}
        }
    }
    has_web_tool
}

/// Custom content kind of `container_upload` blocks.
pub const CONTAINER_UPLOAD_KIND: &str = "anthropic.container_upload";

/// Wraps `value` under the canonical `anthropic` metadata key.
#[must_use]
pub fn anthropic_metadata(value: JsonObject) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    metadata.insert(CANONICAL_OPTIONS_KEY.to_owned(), value);
    metadata
}

fn object(value: JsonValue) -> JsonObject {
    match value {
        JsonValue::Object(object) => object,
        _ => JsonObject::new(),
    }
}

/// Caller metadata (`{caller: {type, toolId?}}`) of a tool use block.
#[must_use]
pub fn caller_metadata(caller: Option<&Caller>) -> Option<ProviderMetadata> {
    let caller = caller?;
    let mut info = JsonObject::new();
    info.insert("type".to_owned(), JsonValue::from(caller.kind.clone()));
    if let Some(tool_id) = &caller.tool_id {
        info.insert("toolId".to_owned(), JsonValue::from(tool_id.clone()));
    }
    let mut value = JsonObject::new();
    value.insert("caller".to_owned(), JsonValue::Object(info));
    Some(anthropic_metadata(value))
}

/// A document that citations may point at (prompt files with citations
/// enabled, then fetched web pages), in order of appearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationDocument {
    /// Title (`filename` or `Untitled Document` for prompt files, page title
    /// or URL for fetched pages).
    pub title: String,
    /// File name of prompt files.
    pub filename: Option<String>,
    /// Media type.
    pub media_type: String,
}

/// Collects the user file parts (PDF or plain text) with citations enabled.
#[must_use]
pub fn citation_documents(
    config: &AnthropicConfig,
    prompt: &[PromptMessage],
) -> Vec<CitationDocument> {
    let mut documents = Vec::new();
    for message in prompt {
        let PromptMessage::User { content, .. } = message else {
            continue;
        };
        for part in content {
            let UserPromptPart::File(file) = part else {
                continue;
            };
            let media_type = file.media_type.as_str();
            if media_type != "application/pdf" && media_type != "text/plain" {
                continue;
            }
            let enabled = read_options::<FilePartOptions>(part_options(
                config,
                file.provider_options.as_ref(),
            ))
            .and_then(|options| options.citations)
            .is_some_and(|citations| citations.enabled);
            if enabled {
                documents.push(CitationDocument {
                    title: file
                        .filename
                        .clone()
                        .unwrap_or_else(|| "Untitled Document".to_owned()),
                    filename: file.filename.clone(),
                    media_type: media_type.to_owned(),
                });
            }
        }
    }
    documents
}

/// Maps content blocks of one message and remembers the state the mapping
/// needs across blocks (MCP tool names, tool search ids, citation
/// documents).
#[derive(Debug)]
pub struct OutputMapper {
    config: SharedConfig,
    mapping: ToolNameMapping,
    /// Whether the JSON response tool replaces native structured output.
    pub uses_json_response_tool: bool,
    /// Whether `code_execution` calls are marked dynamic (a 20260209 web
    /// tool without a code execution tool).
    pub mark_code_execution_dynamic: bool,
    /// Documents citations may reference.
    pub citation_documents: Vec<CitationDocument>,
    /// Set once a `json` tool call was mapped to text.
    pub json_response_from_tool: bool,
    mcp_tool_calls: HashMap<String, (ToolName, Option<ProviderMetadata>)>,
    tool_search_calls: HashMap<String, String>,
}

impl OutputMapper {
    /// Creates a mapper.
    #[must_use]
    pub fn new(config: SharedConfig, mapping: ToolNameMapping) -> Self {
        Self {
            config,
            mapping,
            uses_json_response_tool: false,
            mark_code_execution_dynamic: false,
            citation_documents: Vec::new(),
            json_response_from_tool: false,
            mcp_tool_calls: HashMap::new(),
            tool_search_calls: HashMap::new(),
        }
    }

    /// Custom name of a provider tool.
    #[must_use]
    pub fn custom_name(&self, provider_tool_name: &str) -> ToolName {
        self.mapping.to_custom_tool_name(provider_tool_name).into()
    }

    /// Whether a `code_execution` call is dynamic.
    #[must_use]
    pub fn is_dynamic(&self, provider_tool_name: &str) -> bool {
        self.mark_code_execution_dynamic && provider_tool_name == "code_execution"
    }

    /// Remembers the provider tool name of a tool search call.
    pub fn remember_tool_search(&mut self, id: &str, provider_tool_name: &str) {
        self.tool_search_calls
            .insert(id.to_owned(), provider_tool_name.to_owned());
    }

    /// Converts a citation to a source part.
    #[must_use]
    pub fn citation_source(&self, citation: &JsonValue) -> Option<Source> {
        let kind = citation.get("type").and_then(JsonValue::as_str)?;
        let cited_text = citation
            .get("cited_text")
            .cloned()
            .unwrap_or(JsonValue::Null);
        if kind == "web_search_result_location" {
            let mut meta = JsonObject::new();
            meta.insert("citedText".to_owned(), cited_text);
            meta.insert(
                "encryptedIndex".to_owned(),
                citation
                    .get("encrypted_index")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            );
            return Some(Source::Url {
                id: self.config.generate_id(),
                url: citation.get("url").and_then(JsonValue::as_str)?.to_owned(),
                title: citation
                    .get("title")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned),
                provider_metadata: Some(anthropic_metadata(meta)),
            });
        }
        if kind != "page_location" && kind != "char_location" {
            return None;
        }
        let index =
            usize::try_from(citation.get("document_index").and_then(JsonValue::as_u64)?).ok()?;
        let document = self.citation_documents.get(index)?;
        let mut meta = JsonObject::new();
        meta.insert("citedText".to_owned(), cited_text);
        let keys: [(&str, &str); 2] = if kind == "page_location" {
            [
                ("startPageNumber", "start_page_number"),
                ("endPageNumber", "end_page_number"),
            ]
        } else {
            [
                ("startCharIndex", "start_char_index"),
                ("endCharIndex", "end_char_index"),
            ]
        };
        for (target, source) in keys {
            meta.insert(
                target.to_owned(),
                citation.get(source).cloned().unwrap_or(JsonValue::Null),
            );
        }
        Some(Source::Document {
            id: self.config.generate_id(),
            media_type: MediaType::new(document.media_type.clone()),
            title: citation
                .get("document_title")
                .and_then(JsonValue::as_str)
                .map_or_else(|| document.title.clone(), str::to_owned),
            filename: document.filename.clone(),
            provider_metadata: Some(anthropic_metadata(meta)),
        })
    }

    /// Maps one block of a complete message.
    #[allow(clippy::too_many_lines, reason = "one arm per content block type")]
    pub fn map_block(&mut self, block: &ContentBlock) -> Vec<Content> {
        match block {
            ContentBlock::Text { text, citations } => {
                if self.uses_json_response_tool {
                    return Vec::new();
                }
                let citations = citations.as_deref().unwrap_or(&[]);
                let web: Vec<JsonValue> = citations
                    .iter()
                    .filter(|citation| {
                        citation.get("type").and_then(JsonValue::as_str)
                            == Some("web_search_result_location")
                    })
                    .cloned()
                    .collect();
                let mut content = vec![Content::Text {
                    text: text.clone(),
                    provider_metadata: (!web.is_empty()).then(|| {
                        let mut meta = JsonObject::new();
                        meta.insert("citations".to_owned(), JsonValue::Array(web));
                        anthropic_metadata(meta)
                    }),
                }];
                content.extend(
                    citations
                        .iter()
                        .filter_map(|citation| self.citation_source(citation))
                        .map(Content::Source),
                );
                content
            }
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                let mut meta = JsonObject::new();
                if let Some(signature) = signature {
                    meta.insert("signature".to_owned(), JsonValue::from(signature.clone()));
                }
                vec![Content::Reasoning {
                    text: thinking.clone(),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::RedactedThinking { data } => {
                let mut meta = JsonObject::new();
                meta.insert("redactedData".to_owned(), JsonValue::from(data.clone()));
                vec![Content::Reasoning {
                    text: String::new(),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::ContainerUpload { file_id } => {
                let Ok(kind) = CustomKind::parse(CONTAINER_UPLOAD_KIND) else {
                    return Vec::new();
                };
                let mut meta = JsonObject::new();
                meta.insert("fileId".to_owned(), JsonValue::from(file_id.clone()));
                vec![Content::Custom {
                    kind,
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::Compaction { content } => {
                let mut meta = JsonObject::new();
                meta.insert("type".to_owned(), JsonValue::from("compaction"));
                vec![Content::Text {
                    text: content.clone().unwrap_or_default(),
                    provider_metadata: Some(anthropic_metadata(meta)),
                }]
            }
            ContentBlock::ToolUse {
                id,
                name,
                input,
                caller,
            } => {
                let input = input
                    .clone()
                    .unwrap_or(JsonValue::Object(JsonObject::new()));
                if self.uses_json_response_tool && name == "json" {
                    self.json_response_from_tool = true;
                    return vec![Content::text(input.to_string())];
                }
                let mut call = ToolCall::new(id.as_str(), name.as_str(), input.to_string());
                call.provider_metadata = caller_metadata(caller.as_ref());
                vec![Content::ToolCall(call)]
            }
            ContentBlock::ServerToolUse {
                id,
                name,
                input,
                caller,
            } => self
                .server_tool_use(id, name, input.as_ref(), caller.as_ref())
                .map(Content::ToolCall)
                .into_iter()
                .collect(),
            ContentBlock::McpToolUse {
                id,
                name,
                input,
                server_name,
            } => vec![Content::ToolCall(self.mcp_tool_use(
                id,
                name,
                input.as_ref(),
                server_name,
            ))],
            other => self.map_result_block(other).unwrap_or_default(),
        }
    }

    /// Maps a `server_tool_use` block to a provider-executed tool call.
    #[must_use]
    pub fn server_tool_use(
        &mut self,
        id: &str,
        name: &str,
        input: Option<&JsonValue>,
        caller: Option<&Caller>,
    ) -> Option<ToolCall> {
        let input = input
            .cloned()
            .unwrap_or(JsonValue::Object(JsonObject::new()));
        let (provider_name, input) = match name {
            "text_editor_code_execution" | "bash_code_execution" => {
                let mut typed = JsonObject::new();
                typed.insert("type".to_owned(), JsonValue::from(name));
                typed.extend(object(input));
                ("code_execution", JsonValue::Object(typed))
            }
            "code_execution" => ("code_execution", programmatic_tool_call(input)),
            "web_search" | "web_fetch" => (name, input),
            "tool_search_tool_regex" | "tool_search_tool_bm25" => {
                self.remember_tool_search(id, name);
                (name, input)
            }
            "advisor" => ("advisor", input),
            _ => return None,
        };
        let mut call = ToolCall::new(id, self.custom_name(provider_name), input.to_string());
        call.provider_executed = true;
        call.dynamic = self.is_dynamic(provider_name);
        call.provider_metadata = caller_metadata(caller);
        Some(call)
    }

    /// Maps an `mcp_tool_use` block and remembers its name for the result.
    #[must_use]
    pub fn mcp_tool_use(
        &mut self,
        id: &str,
        name: &str,
        input: Option<&JsonValue>,
        server_name: &str,
    ) -> ToolCall {
        let input = input
            .cloned()
            .unwrap_or(JsonValue::Object(JsonObject::new()));
        let mut meta = JsonObject::new();
        meta.insert("type".to_owned(), JsonValue::from("mcp-tool-use"));
        meta.insert("serverName".to_owned(), JsonValue::from(server_name));
        let mut call = ToolCall::new(id, name, input.to_string());
        call.provider_executed = true;
        call.dynamic = true;
        call.provider_metadata = Some(anthropic_metadata(meta));
        self.mcp_tool_calls.insert(
            id.to_owned(),
            (call.tool_name.clone(), call.provider_metadata.clone()),
        );
        call
    }
}

/// Injects `type: programmatic-tool-call` into a code execution input that
/// carries `code` but no `type`.
#[must_use]
pub fn programmatic_tool_call(input: JsonValue) -> JsonValue {
    match input {
        JsonValue::Object(object)
            if object.contains_key("code") && !object.contains_key("type") =>
        {
            let mut typed = JsonObject::new();
            typed.insert("type".to_owned(), JsonValue::from("programmatic-tool-call"));
            typed.extend(object);
            JsonValue::Object(typed)
        }
        other => other,
    }
}
