//! Mapping of `generateContent` responses to specification content, usage,
//! finish reasons and provider metadata.

use base64::Engine;
use bytes::Bytes;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::Content;
use ferrin_spec::FileData;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::InputTokens;
use ferrin_spec::language_model::OutputTokens;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_spec::language_model::Source;
use serde_json::json;

use crate::api_types::Candidate;
use crate::api_types::GenerateContentResponse;
use crate::api_types::GroundingChunk;
use crate::api_types::Part;
use crate::api_types::UsageMetadata;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::SharedConfig;
use crate::prepare_tools::CODE_EXECUTION_TOOL_NAME;

/// Maps a Gemini finish reason.
#[must_use]
pub fn map_finish_reason(reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
    let unified = match reason {
        Some("STOP") if has_tool_calls => FinishReasonKind::ToolCalls,
        Some("STOP") => FinishReasonKind::Stop,
        Some("MAX_TOKENS") => FinishReasonKind::Length,
        Some(
            "IMAGE_SAFETY" | "RECITATION" | "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII",
        ) => FinishReasonKind::ContentFilter,
        Some("MALFORMED_FUNCTION_CALL") => FinishReasonKind::Error,
        _ => FinishReasonKind::Other,
    };
    match reason {
        Some(raw) => FinishReason::with_raw(unified, raw),
        None => FinishReason::new(unified),
    }
}

/// Converts `usageMetadata`; `raw` is the original object.
#[must_use]
pub fn convert_usage(usage: Option<&UsageMetadata>, raw: Option<JsonObject>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let prompt = usage.prompt_token_count.unwrap_or_default();
    let candidates = usage.candidates_token_count.unwrap_or_default();
    let cached = usage.cached_content_token_count.unwrap_or_default();
    let thoughts = usage.thoughts_token_count.unwrap_or_default();
    Usage {
        input: InputTokens {
            total: Some(prompt),
            no_cache: Some(prompt.saturating_sub(cached)),
            cache_read: Some(cached),
            cache_write: None,
        },
        output: OutputTokens {
            total: Some(candidates + thoughts),
            text: Some(candidates),
            reasoning: Some(thoughts),
        },
        raw,
    }
}

/// Whether `content` contains a tool call executed by the client.
#[must_use]
pub fn has_client_tool_calls(content: &[Content]) -> bool {
    content
        .iter()
        .any(|part| matches!(part, Content::ToolCall(call) if !call.provider_executed))
}

/// Media type of a retrieved document by file extension.
#[must_use]
pub fn document_media_type(uri: &str) -> &'static str {
    let lower = uri.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else if lower.ends_with(".doc") {
        "application/msword"
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
        "text/markdown"
    } else {
        "application/octet-stream"
    }
}

fn last_segment(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
}

fn decode_base64(data: &str) -> Result<Bytes, ProviderError> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map(Bytes::from)
        .map_err(|error| {
            ProviderError::InvalidResponseData(Box::new(InvalidResponseDataError::new(
                format!("invalid base64 inline data: {error}"),
                JsonValue::Null,
            )))
        })
}

/// Maps response parts to content and builds provider metadata.
#[derive(Debug, Clone)]
pub struct OutputMapper {
    config: SharedConfig,
    mapping: ToolNameMapping,
    last_code_execution_id: Option<String>,
    last_server_tool_call_id: Option<String>,
}

impl OutputMapper {
    /// Creates a mapper.
    #[must_use]
    pub fn new(config: SharedConfig, mapping: ToolNameMapping) -> Self {
        Self {
            config,
            mapping,
            last_code_execution_id: None,
            last_server_tool_call_id: None,
        }
    }

    /// Wraps `object` under the `google` key (and the configured name when it
    /// differs).
    #[must_use]
    pub fn metadata(&self, object: JsonObject) -> ProviderMetadata {
        let mut metadata = ProviderMetadata::new();
        if self.config.options_key() != CANONICAL_OPTIONS_KEY {
            metadata.insert(self.config.options_key().to_owned(), object.clone());
        }
        metadata.insert(CANONICAL_OPTIONS_KEY.to_owned(), object);
        metadata
    }

    /// Like [`Self::metadata`], `None` for an empty object.
    #[must_use]
    pub fn metadata_opt(&self, object: JsonObject) -> Option<ProviderMetadata> {
        (!object.is_empty()).then(|| self.metadata(object))
    }

    /// Metadata carrying only a thought signature.
    #[must_use]
    pub fn signature_metadata(&self, signature: Option<&str>) -> Option<ProviderMetadata> {
        signature.map(|signature| {
            let mut object = JsonObject::new();
            object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
            self.metadata(object)
        })
    }

    /// Custom name of the code execution tool.
    #[must_use]
    pub fn code_execution_name(&self) -> String {
        self.mapping
            .to_custom_tool_name(CODE_EXECUTION_TOOL_NAME)
            .to_owned()
    }

    /// Custom name of a function.
    #[must_use]
    pub fn custom_tool_name(&self, provider_name: &str) -> String {
        self.mapping.to_custom_tool_name(provider_name).to_owned()
    }

    /// Generates an id.
    #[must_use]
    pub fn generate_id(&self) -> String {
        self.config.generate_id()
    }

    /// Maps a code execution part to a provider-executed tool call.
    #[must_use]
    pub fn code_execution_call(&mut self, language: Option<&str>, code: Option<&str>) -> ToolCall {
        self.code_execution_call_with_signature(language, code, None)
    }

    pub(crate) fn code_execution_call_with_signature(
        &mut self,
        language: Option<&str>,
        code: Option<&str>,
        signature: Option<&str>,
    ) -> ToolCall {
        let id = self.generate_id();
        self.last_code_execution_id = Some(id.clone());
        let mut call = ToolCall::new(
            id,
            self.code_execution_name(),
            json!({"language": language, "code": code}).to_string(),
        );
        call.provider_executed = true;
        call.provider_metadata = Some(self.code_execution_metadata(signature));
        call
    }

    /// Maps a code execution result.
    #[must_use]
    pub fn code_execution_result(
        &mut self,
        outcome: Option<&str>,
        output: Option<&str>,
    ) -> ProviderToolResult {
        self.code_execution_result_with_signature(outcome, output, None)
    }

    pub(crate) fn code_execution_result_with_signature(
        &mut self,
        outcome: Option<&str>,
        output: Option<&str>,
        signature: Option<&str>,
    ) -> ProviderToolResult {
        let id = self
            .last_code_execution_id
            .take()
            .unwrap_or_else(|| self.generate_id());
        ProviderToolResult {
            tool_call_id: id.into(),
            tool_name: self.code_execution_name().into(),
            result: json!({"outcome": outcome, "output": output.unwrap_or_default()}),
            is_error: false,
            preliminary: false,
            dynamic: false,
            provider_metadata: Some(self.code_execution_metadata(signature)),
        }
    }

    fn code_execution_metadata(&self, signature: Option<&str>) -> ProviderMetadata {
        let mut object = JsonObject::new();
        object.insert(
            "serverToolType".to_owned(),
            JsonValue::from("code_execution"),
        );
        if let Some(signature) = signature {
            object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
        }
        self.metadata(object)
    }

    /// Maps a function call part.
    #[must_use]
    pub fn function_call(
        &self,
        id: Option<&str>,
        name: &str,
        args: Option<&JsonValue>,
        signature: Option<&str>,
    ) -> ToolCall {
        let id = id
            .filter(|id| !id.is_empty())
            .map_or_else(|| self.generate_id(), str::to_owned);
        let input = args
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(JsonObject::new()))
            .to_string();
        let mut call = ToolCall::new(id, self.custom_tool_name(name), input);
        call.provider_metadata = self.signature_metadata(signature);
        call
    }

    /// Maps a server-side tool call part.
    #[must_use]
    pub fn server_tool_call(
        &mut self,
        tool_type: Option<&str>,
        args: Option<&JsonValue>,
        id: Option<&str>,
        signature: Option<&str>,
    ) -> ToolCall {
        let tool_type = tool_type.unwrap_or("unknown");
        let id = id
            .filter(|id| !id.is_empty())
            .map_or_else(|| self.generate_id(), str::to_owned);
        self.last_server_tool_call_id = Some(id.clone());
        let mut call = ToolCall::new(
            id.clone(),
            format!("server:{tool_type}"),
            args.cloned()
                .unwrap_or_else(|| JsonValue::Object(JsonObject::new()))
                .to_string(),
        );
        call.provider_executed = true;
        call.dynamic = true;
        let mut object = JsonObject::new();
        if let Some(signature) = signature {
            object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
        }
        object.insert("serverToolCallId".to_owned(), JsonValue::from(id));
        object.insert("serverToolType".to_owned(), JsonValue::from(tool_type));
        call.provider_metadata = Some(self.metadata(object));
        call
    }

    /// Maps a server-side tool response part.
    #[must_use]
    pub fn server_tool_response(&mut self, response: &JsonValue) -> ProviderToolResult {
        let tool_type = response
            .get("toolType")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        let id = self
            .last_server_tool_call_id
            .take()
            .or_else(|| {
                response
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| self.generate_id());
        ProviderToolResult {
            tool_call_id: id.into(),
            tool_name: format!("server:{tool_type}").into(),
            result: response.clone(),
            is_error: false,
            preliminary: false,
            dynamic: true,
            provider_metadata: None,
        }
    }

    /// Decodes an inline data part into a file or reasoning file.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidResponseData`] for invalid base64.
    pub fn inline_file(
        &self,
        mime_type: &str,
        data: &str,
        thought: bool,
        signature: Option<&str>,
    ) -> Result<Content, ProviderError> {
        let bytes = decode_base64(data)?;
        let media_type = MediaType::new(mime_type);
        Ok(if thought {
            Content::ReasoningFile {
                data: FileData::Bytes { data: bytes },
                media_type,
                provider_metadata: self.signature_metadata(signature),
            }
        } else {
            Content::File {
                data: FileData::Bytes { data: bytes },
                media_type,
                filename: None,
                provider_metadata: self.signature_metadata(signature),
            }
        })
    }

    /// Maps the parts of a non-streaming candidate.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidResponseData`] for invalid inline data.
    pub fn map_parts(&mut self, parts: &[Part]) -> Result<Vec<Content>, ProviderError> {
        let mut content: Vec<Content> = Vec::new();
        for part in parts {
            let signature = part.thought_signature.as_deref();
            if let Some(code) = &part.executable_code {
                content.push(Content::ToolCall(self.code_execution_call_with_signature(
                    code.language.as_deref(),
                    code.code.as_deref(),
                    signature,
                )));
            }
            if let Some(result) = &part.code_execution_result {
                content.push(Content::ToolResult(
                    self.code_execution_result_with_signature(
                        result.outcome.as_deref(),
                        result.output.as_deref(),
                        signature,
                    ),
                ));
            }
            if let Some(text) = &part.text {
                if text.is_empty() {
                    if let Some(signature) = signature {
                        attach_signature(&mut content, self.signature_metadata(Some(signature)));
                    }
                } else if part.thought == Some(true) {
                    content.push(Content::Reasoning {
                        text: text.clone(),
                        provider_metadata: self.signature_metadata(signature),
                    });
                } else {
                    content.push(Content::Text {
                        text: text.clone(),
                        provider_metadata: self.signature_metadata(signature),
                    });
                }
            }
            if let Some(call) = &part.function_call
                && let Some(name) = &call.name
            {
                content.push(Content::ToolCall(self.function_call(
                    call.id.as_deref(),
                    name,
                    call.args.as_ref(),
                    signature,
                )));
            }
            if let Some(inline) = &part.inline_data {
                content.push(self.inline_file(
                    &inline.mime_type,
                    &inline.data,
                    part.thought == Some(true),
                    signature,
                )?);
            }
            if let Some(call) = &part.tool_call {
                content.push(Content::ToolCall(self.server_tool_call(
                    call.tool_type.as_deref(),
                    call.args.as_ref(),
                    call.id.as_deref(),
                    signature,
                )));
            }
            if let Some(response) = &part.tool_response {
                content.push(Content::ToolResult(self.server_tool_response(response)));
            }
        }
        Ok(content)
    }

    /// Maps grounding chunks to sources.
    #[must_use]
    pub fn sources(&self, chunks: &[GroundingChunk]) -> Vec<Source> {
        let mut sources = Vec::new();
        for chunk in chunks {
            if let Some(web) = &chunk.web
                && let Some(uri) = &web.uri
            {
                sources.push(Source::Url {
                    id: self.generate_id(),
                    url: uri.clone(),
                    title: web.title.clone(),
                    provider_metadata: None,
                });
            }
            if let Some(image) = &chunk.image
                && let Some(uri) = image.source_uri.as_ref().or(image.image_uri.as_ref())
            {
                sources.push(Source::Url {
                    id: self.generate_id(),
                    url: uri.clone(),
                    title: image.title.clone(),
                    provider_metadata: None,
                });
            }
            if let Some(context) = &chunk.retrieved_context {
                if let Some(uri) = &context.uri {
                    if uri.starts_with("http://") || uri.starts_with("https://") {
                        sources.push(Source::Url {
                            id: self.generate_id(),
                            url: uri.clone(),
                            title: context.title.clone(),
                            provider_metadata: None,
                        });
                    } else {
                        sources.push(Source::Document {
                            id: self.generate_id(),
                            media_type: MediaType::new(document_media_type(uri)),
                            title: context
                                .title
                                .clone()
                                .unwrap_or_else(|| "Unknown Document".to_owned()),
                            filename: last_segment(uri),
                            provider_metadata: None,
                        });
                    }
                } else if let Some(store) = &context.file_search_store {
                    sources.push(Source::Document {
                        id: self.generate_id(),
                        media_type: MediaType::new("application/octet-stream"),
                        title: context
                            .title
                            .clone()
                            .unwrap_or_else(|| "Unknown Document".to_owned()),
                        filename: last_segment(store),
                        provider_metadata: None,
                    });
                }
            }
            if let Some(maps) = &chunk.maps
                && let Some(uri) = &maps.uri
            {
                sources.push(Source::Url {
                    id: self.generate_id(),
                    url: uri.clone(),
                    title: maps.title.clone(),
                    provider_metadata: None,
                });
            }
        }
        sources
    }

    /// Result-level provider metadata.
    #[must_use]
    pub fn response_metadata(
        &self,
        response: &GenerateContentResponse,
        candidate: Option<&Candidate>,
        raw_usage: Option<&JsonValue>,
    ) -> ProviderMetadata {
        let mut object = JsonObject::new();
        object.insert(
            "promptFeedback".to_owned(),
            response.prompt_feedback.clone().unwrap_or(JsonValue::Null),
        );
        object.insert(
            "groundingMetadata".to_owned(),
            candidate
                .and_then(|candidate| candidate.grounding_metadata.clone())
                .unwrap_or(JsonValue::Null),
        );
        object.insert(
            "urlContextMetadata".to_owned(),
            candidate
                .and_then(|candidate| candidate.url_context_metadata.clone())
                .unwrap_or(JsonValue::Null),
        );
        object.insert(
            "safetyRatings".to_owned(),
            candidate
                .and_then(|candidate| candidate.safety_ratings.clone())
                .unwrap_or(JsonValue::Null),
        );
        object.insert(
            "usageMetadata".to_owned(),
            raw_usage.cloned().unwrap_or(JsonValue::Null),
        );
        object.insert(
            "finishMessage".to_owned(),
            candidate
                .and_then(|candidate| candidate.finish_message.clone())
                .map_or(JsonValue::Null, JsonValue::from),
        );
        object.insert(
            "serviceTier".to_owned(),
            response
                .usage_metadata
                .as_ref()
                .and_then(|usage| usage.service_tier.clone())
                .map_or(JsonValue::Null, JsonValue::from),
        );
        self.metadata(object)
    }
}

/// Attaches `metadata` to the last content part that can carry it.
fn attach_signature(content: &mut [Content], metadata: Option<ProviderMetadata>) {
    let Some(last) = content.last_mut() else {
        return;
    };
    match last {
        Content::Text {
            provider_metadata, ..
        }
        | Content::Reasoning {
            provider_metadata, ..
        }
        | Content::File {
            provider_metadata, ..
        }
        | Content::ReasoningFile {
            provider_metadata, ..
        } => merge_metadata(provider_metadata, metadata),
        Content::ToolCall(call) => merge_metadata(&mut call.provider_metadata, metadata),
        _ => {}
    }
}

fn merge_metadata(target: &mut Option<ProviderMetadata>, extra: Option<ProviderMetadata>) {
    let Some(extra) = extra else {
        return;
    };
    match target {
        Some(existing) => {
            for (key, object) in extra {
                existing.entry(key).or_default().extend(object);
            }
        }
        None => *target = Some(extra),
    }
}

/// `usageMetadata` object of a raw response.
#[must_use]
pub fn raw_usage(raw: Option<&JsonValue>) -> Option<JsonObject> {
    raw?.get("usageMetadata")?.as_object().cloned()
}
