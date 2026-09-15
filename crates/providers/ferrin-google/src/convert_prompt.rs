//! Conversion of the specification prompt to `systemInstruction` and
//! `contents`.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use base64::Engine;
use ferrin_provider_util::media_type::resolve_full_media_type;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::NoSuchProviderReferenceError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use ferrin_spec::shared::Warning;
use serde_json::json;

use crate::capabilities::ModelCapabilities;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::GoogleConfig;
use crate::options::read_part_options;

/// Signature sent for Gemini 3 tool calls that lack one.
pub const SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

/// Result of the prompt conversion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedPrompt {
    /// `systemInstruction` (`{parts: [{text}]}`), absent without system
    /// messages or for Gemma models (whose system text is prepended to the
    /// first user part).
    pub system_instruction: Option<JsonObject>,
    /// `contents`.
    pub contents: Vec<JsonValue>,
    /// Warnings collected during the conversion.
    pub warnings: Vec<Warning>,
}

/// Resolves a file reference under the configured name, then `google`.
///
/// # Errors
///
/// Returns [`NoSuchProviderReferenceError`] when neither key is present.
pub fn resolve_reference<'a>(
    config: &GoogleConfig,
    reference: &'a ProviderReference,
) -> Result<&'a str, NoSuchProviderReferenceError> {
    reference
        .get(config.options_key())
        .or_else(|| reference.get(CANONICAL_OPTIONS_KEY))
        .map(String::as_str)
        .ok_or_else(|| NoSuchProviderReferenceError::new(config.name.clone(), reference.clone()))
}

fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn inline_data(media_type: &str, data: &str) -> JsonValue {
    json!({"inlineData": {"mimeType": media_type, "data": data}})
}

fn file_data(media_type: &str, uri: &str) -> JsonValue {
    json!({"fileData": {"mimeType": media_type, "fileUri": uri}})
}

fn full_media_type(media_type: &MediaType, bytes: Option<&[u8]>) -> Result<String, ProviderError> {
    Ok(resolve_full_media_type(media_type, bytes)?.into_string())
}

/// Converts a file to a `fileData` or `inlineData` part.
fn file_part(
    config: &GoogleConfig,
    data: &FileData,
    media_type: &MediaType,
    in_assistant: bool,
) -> Result<JsonValue, ProviderError> {
    match data {
        FileData::Url { url } => {
            if in_assistant {
                return Err(UnsupportedFunctionalityError::new(
                    "File data URLs in assistant messages are not supported",
                )
                .into());
            }
            Ok(file_data(&full_media_type(media_type, None)?, url.as_str()))
        }
        FileData::Reference { reference } => Ok(file_data(
            &full_media_type(media_type, None)?,
            resolve_reference(config, reference)?,
        )),
        FileData::Bytes { data } => Ok(inline_data(
            &full_media_type(media_type, Some(data.as_ref()))?,
            &encode_base64(data.as_ref()),
        )),
        FileData::Text { text } => {
            let media_type = if media_type.is_full() {
                media_type.as_str().to_owned()
            } else {
                "text/plain".to_owned()
            };
            Ok(inline_data(&media_type, &encode_base64(text.as_bytes())))
        }
        #[allow(unreachable_patterns, reason = "FileData is non-exhaustive")]
        _ => Err(UnsupportedFunctionalityError::new("file data variant").into()),
    }
}

fn with_signature(mut part: JsonValue, signature: Option<&str>) -> JsonValue {
    if let (Some(signature), Some(object)) = (signature, part.as_object_mut()) {
        object.insert("thoughtSignature".to_owned(), JsonValue::from(signature));
    }
    part
}

fn with_thought(mut part: JsonValue) -> JsonValue {
    if let Some(object) = part.as_object_mut() {
        object.insert("thought".to_owned(), JsonValue::Bool(true));
    }
    part
}

fn tool_result_value(output: &ToolResultOutput) -> JsonValue {
    match output {
        ToolResultOutput::Text { value, .. } | ToolResultOutput::ErrorText { value, .. } => {
            JsonValue::from(value.as_str())
        }
        ToolResultOutput::Json { value, .. } | ToolResultOutput::ErrorJson { value, .. } => {
            value.clone()
        }
        ToolResultOutput::ExecutionDenied { reason, .. } => JsonValue::from(
            reason
                .clone()
                .unwrap_or_else(|| "Tool call execution denied.".to_owned()),
        ),
        ToolResultOutput::Content { value, .. } => JsonValue::Array(
            value
                .iter()
                .filter_map(|part| match part {
                    ToolResultContentPart::Text { text, .. } => {
                        Some(JsonValue::from(text.as_str()))
                    }
                    _ => None,
                })
                .collect(),
        ),
        #[allow(unreachable_patterns, reason = "ToolResultOutput is non-exhaustive")]
        _ => JsonValue::Null,
    }
}

fn function_response(id: Option<&str>, name: &str, response: JsonValue) -> JsonObject {
    let mut function_response = JsonObject::new();
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        function_response.insert("id".to_owned(), JsonValue::from(id));
    }
    function_response.insert("name".to_owned(), JsonValue::from(name));
    function_response.insert("response".to_owned(), response);
    function_response
}

/// Decodes a base64 `data:` URL into `(media type, base64 payload)`.
fn parse_data_url(url: &url::Url) -> Option<(String, String)> {
    let rest = url.as_str().strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    let mut segments = header.split(';');
    let media_type = segments.next().unwrap_or_default().to_owned();
    if segments.any(|segment| segment.eq_ignore_ascii_case("base64")) {
        Some((media_type, payload.to_owned()))
    } else {
        let decoded = percent_decode(payload);
        Some((media_type, encode_base64(decoded.as_bytes())))
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

struct Converter<'a> {
    config: &'a GoogleConfig,
    capabilities: ModelCapabilities,
    mapping: &'a ToolNameMapping,
    warnings: Vec<Warning>,
    contents: Vec<JsonValue>,
}

impl Converter<'_> {
    fn push_content(&mut self, role: &str, parts: Vec<JsonValue>) {
        if !parts.is_empty() {
            self.contents.push(json!({"role": role, "parts": parts}));
        }
    }

    /// Appends `part` to the last model content, or opens a new one.
    fn append_to_model(&mut self, part: JsonValue) {
        if let Some(JsonValue::Object(last)) = self.contents.last_mut()
            && last.get("role").and_then(JsonValue::as_str) == Some("model")
            && let Some(JsonValue::Array(parts)) = last.get_mut("parts")
        {
            parts.push(part);
            return;
        }
        self.contents
            .push(json!({"role": "model", "parts": [part]}));
    }

    fn user_message(&mut self, content: &[UserPromptPart]) -> Result<(), ProviderError> {
        let mut parts = Vec::new();
        for part in content {
            match part {
                UserPromptPart::Text(text) => {
                    parts.push(json!({"text": text.text}));
                }
                UserPromptPart::File(file) => {
                    parts.push(file_part(self.config, &file.data, &file.media_type, false)?);
                }
                #[allow(unreachable_patterns, reason = "UserPromptPart is non-exhaustive")]
                _ => self
                    .warnings
                    .push(Warning::other("unknown user part ignored")),
            }
        }
        self.push_content("user", parts);
        Ok(())
    }

    #[allow(clippy::too_many_lines, reason = "one arm per assistant part kind")]
    fn assistant_message(&mut self, content: &[AssistantPromptPart]) -> Result<(), ProviderError> {
        let mut parts = Vec::new();
        let mut has_signed_call = false;
        let mut unsigned_calls: Vec<String> = Vec::new();
        for part in content {
            match part {
                AssistantPromptPart::Text(text) => {
                    if text.text.is_empty() {
                        continue;
                    }
                    let options = read_part_options(self.config, text.provider_options.as_ref());
                    parts.push(with_signature(
                        json!({"text": text.text}),
                        options.thought_signature.as_deref(),
                    ));
                }
                AssistantPromptPart::Reasoning(reasoning) => {
                    let options =
                        read_part_options(self.config, reasoning.provider_options.as_ref());
                    parts.push(with_signature(
                        json!({"text": reasoning.text, "thought": true}),
                        options.thought_signature.as_deref(),
                    ));
                }
                AssistantPromptPart::ReasoningFile(file) => {
                    let options = read_part_options(self.config, file.provider_options.as_ref());
                    parts.push(with_signature(
                        with_thought(file_part(self.config, &file.data, &file.media_type, true)?),
                        options.thought_signature.as_deref(),
                    ));
                }
                AssistantPromptPart::File(file) => {
                    let options = read_part_options(self.config, file.provider_options.as_ref());
                    let converted = file_part(self.config, &file.data, &file.media_type, true)?;
                    let converted = if options.thought == Some(true) {
                        with_thought(converted)
                    } else {
                        converted
                    };
                    parts.push(with_signature(
                        converted,
                        options.thought_signature.as_deref(),
                    ));
                }
                AssistantPromptPart::Custom(_) => {
                    self.warnings.push(Warning::other(
                        "custom assistant parts are not supported and were ignored",
                    ));
                }
                AssistantPromptPart::ToolCall(call) => {
                    let options = read_part_options(self.config, call.provider_options.as_ref());
                    if let Some(tool_type) = &options.server_tool_type {
                        let mut tool_call = JsonObject::new();
                        tool_call
                            .insert("toolType".to_owned(), JsonValue::from(tool_type.as_str()));
                        tool_call.insert("args".to_owned(), call.input.clone());
                        if let Some(id) = &options.server_tool_call_id {
                            tool_call.insert("id".to_owned(), JsonValue::from(id.as_str()));
                        }
                        parts.push(with_signature(
                            json!({"toolCall": tool_call}),
                            options.thought_signature.as_deref(),
                        ));
                        continue;
                    }
                    let mut function_call = JsonObject::new();
                    if !call.tool_call_id.as_str().is_empty() {
                        function_call
                            .insert("id".to_owned(), JsonValue::from(call.tool_call_id.as_str()));
                    }
                    function_call.insert(
                        "name".to_owned(),
                        JsonValue::from(
                            self.mapping.to_provider_tool_name(call.tool_name.as_str()),
                        ),
                    );
                    function_call.insert("args".to_owned(), call.input.clone());
                    let signature = match &options.thought_signature {
                        Some(signature) => {
                            has_signed_call = true;
                            Some(signature.clone())
                        }
                        None if self.capabilities.uses_gemini3_features && !has_signed_call => {
                            unsigned_calls.push(call.tool_name.as_str().to_owned());
                            Some(SKIP_THOUGHT_SIGNATURE.to_owned())
                        }
                        None => None,
                    };
                    parts.push(with_signature(
                        json!({"functionCall": function_call}),
                        signature.as_deref(),
                    ));
                }
                AssistantPromptPart::ToolResult(result) => {
                    let options = read_part_options(self.config, result.provider_options.as_ref());
                    if let Some(tool_type) = &options.server_tool_type {
                        let mut response = JsonObject::new();
                        response.insert("toolType".to_owned(), JsonValue::from(tool_type.as_str()));
                        if let Some(id) = &options.server_tool_call_id {
                            response.insert("id".to_owned(), JsonValue::from(id.as_str()));
                        }
                        response.insert("response".to_owned(), tool_result_value(&result.output));
                        parts.push(json!({"toolResponse": response}));
                    }
                }
                #[allow(unreachable_patterns, reason = "AssistantPromptPart is non-exhaustive")]
                _ => self
                    .warnings
                    .push(Warning::other("unknown assistant part ignored")),
            }
        }
        if !unsigned_calls.is_empty() {
            self.warnings.push(Warning::other(format!(
                "thought signatures are missing for tool call(s) {}; \"{SKIP_THOUGHT_SIGNATURE}\" was sent instead, which may reduce response quality",
                unsigned_calls.join(", ")
            )));
        }
        self.push_content("model", parts);
        Ok(())
    }

    fn tool_message(&mut self, content: &[ToolPromptPart]) -> Result<(), ProviderError> {
        let mut parts = Vec::new();
        for part in content {
            match part {
                ToolPromptPart::ToolApprovalResponse(_) => {}
                ToolPromptPart::ToolResult(result) => {
                    let options = read_part_options(self.config, result.provider_options.as_ref());
                    if let Some(tool_type) = &options.server_tool_type {
                        let mut response = JsonObject::new();
                        response.insert("toolType".to_owned(), JsonValue::from(tool_type.as_str()));
                        if let Some(id) = &options.server_tool_call_id {
                            response.insert("id".to_owned(), JsonValue::from(id.as_str()));
                        }
                        response.insert("response".to_owned(), tool_result_value(&result.output));
                        self.append_to_model(json!({"toolResponse": response}));
                        continue;
                    }
                    self.function_result(result, &mut parts)?;
                }
                #[allow(unreachable_patterns, reason = "ToolPromptPart is non-exhaustive")]
                _ => self
                    .warnings
                    .push(Warning::other("unknown tool part ignored")),
            }
        }
        self.push_content("user", parts);
        Ok(())
    }

    fn function_result(
        &mut self,
        result: &ToolResultPart,
        parts: &mut Vec<JsonValue>,
    ) -> Result<(), ProviderError> {
        let name = self
            .mapping
            .to_provider_tool_name(result.tool_name.as_str())
            .to_owned();
        let id = Some(result.tool_call_id.as_str());
        let ToolResultOutput::Content { value, .. } = &result.output else {
            let response = json!({"name": name, "content": tool_result_value(&result.output)});
            parts.push(json!({"functionResponse": function_response(id, &name, response)}));
            return Ok(());
        };
        let mut texts: Vec<&str> = Vec::new();
        let mut files: Vec<(String, String)> = Vec::new();
        for part in value {
            match part {
                ToolResultContentPart::Text { text, .. } => texts.push(text),
                ToolResultContentPart::File {
                    data, media_type, ..
                } => match data {
                    FileData::Bytes { data } => files.push((
                        full_media_type(media_type, Some(data.as_ref()))?,
                        encode_base64(data.as_ref()),
                    )),
                    FileData::Url { url } if url.scheme() == "data" => {
                        if let Some((mime, payload)) = parse_data_url(url) {
                            files.push((mime, payload));
                        }
                    }
                    FileData::Text { text } => {
                        files.push(("text/plain".to_owned(), encode_base64(text.as_bytes())));
                    }
                    _ => self.warnings.push(Warning::unsupported_with_details(
                        "tool result file",
                        "tool result files must be provided as bytes or data URLs; the part was ignored",
                    )),
                },
                ToolResultContentPart::Custom { .. } => {}
                #[allow(unreachable_patterns, reason = "ToolResultContentPart is non-exhaustive")]
                _ => {}
            }
        }
        if self.capabilities.uses_gemini3_features {
            let content = if texts.is_empty() {
                "Tool executed successfully.".to_owned()
            } else {
                texts.join("\n")
            };
            let mut function_response =
                function_response(id, &name, json!({"name": name, "content": content}));
            if !files.is_empty() {
                function_response.insert(
                    "parts".to_owned(),
                    JsonValue::Array(
                        files
                            .iter()
                            .map(|(mime, data)| inline_data(mime, data))
                            .collect(),
                    ),
                );
            }
            parts.push(json!({"functionResponse": function_response}));
            return Ok(());
        }
        for text in texts {
            parts.push(json!({"functionResponse": function_response(
                id,
                &name,
                json!({"name": name, "content": text}),
            )}));
        }
        for (mime, data) in files {
            let kind = if mime.starts_with("image/") {
                "image"
            } else {
                "file"
            };
            parts.push(inline_data(&mime, &data));
            parts.push(json!({
                "text": format!("Tool executed successfully and returned this {kind} as a response")
            }));
        }
        Ok(())
    }
}

fn message_options(message: &PromptMessage) -> Option<&ProviderOptions> {
    message.provider_options()
}

/// Converts `prompt` to Gemini `contents` and `systemInstruction`.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for system messages
/// after the first non-system message, for assistant file URLs and for
/// media types without a resolvable subtype, and
/// [`ProviderError::NoSuchProviderReference`] for file references without a
/// key for this provider.
pub fn convert_prompt(
    config: &GoogleConfig,
    prompt: &[PromptMessage],
    capabilities: ModelCapabilities,
    mapping: &ToolNameMapping,
) -> Result<ConvertedPrompt, ProviderError> {
    let mut converter = Converter {
        config,
        capabilities,
        mapping,
        warnings: Vec::new(),
        contents: Vec::new(),
    };
    let mut system_parts: Vec<JsonValue> = Vec::new();
    let mut system_allowed = true;
    for message in prompt {
        let _ = message_options(message);
        match message {
            PromptMessage::System { content, .. } => {
                if !system_allowed {
                    return Err(UnsupportedFunctionalityError::new(
                        "system messages are only supported at the beginning of the conversation",
                    )
                    .into());
                }
                system_parts.push(json!({"text": content}));
            }
            PromptMessage::User { content, .. } => {
                system_allowed = false;
                converter.user_message(content)?;
            }
            PromptMessage::Assistant { content, .. } => {
                system_allowed = false;
                converter.assistant_message(content)?;
            }
            PromptMessage::Tool { content, .. } => {
                system_allowed = false;
                converter.tool_message(content)?;
            }
            #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
            _ => converter
                .warnings
                .push(Warning::other("unknown message role ignored")),
        }
    }
    let mut system_instruction = None;
    if !system_parts.is_empty() {
        if capabilities.is_gemma {
            prepend_system_text(&mut converter.contents, &system_parts);
        } else {
            let mut instruction = JsonObject::new();
            instruction.insert("parts".to_owned(), JsonValue::Array(system_parts));
            system_instruction = Some(instruction);
        }
    }
    Ok(ConvertedPrompt {
        system_instruction,
        contents: converter.contents,
        warnings: converter.warnings,
    })
}

/// Gemma models reject `systemInstruction`: the system text is prepended to
/// the first user text part instead.
fn prepend_system_text(contents: &mut [JsonValue], system_parts: &[JsonValue]) {
    let system_text = system_parts
        .iter()
        .filter_map(|part| part.get("text").and_then(JsonValue::as_str))
        .collect::<Vec<_>>()
        .join("\n\n");
    let Some(first_user) = contents
        .iter_mut()
        .find(|content| content.get("role").and_then(JsonValue::as_str) == Some("user"))
    else {
        return;
    };
    let Some(JsonValue::Array(parts)) = first_user.get_mut("parts") else {
        return;
    };
    match parts.first_mut().and_then(|part| part.get_mut("text")) {
        Some(JsonValue::String(text)) => {
            *text = format!("{system_text}\n\n{text}");
        }
        _ => parts.insert(0, json!({"text": system_text})),
    }
}
