//! Conversion of the specification prompt to Responses API input items.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.
//!
//! Advanced tool behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use std::collections::HashMap;
use std::collections::HashSet;

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;

use super::convert_tool_results::convert_tool_message;
use super::options::PartOptions;
use crate::capabilities::SystemMessageMode;

/// Provider-defined tools present in the request that change how tool calls
/// and results are converted.
#[derive(Debug, Clone, Default)]
pub struct ProviderToolSet {
    /// `openai.tool_search` is declared.
    pub tool_search: bool,
    /// `openai.programmatic_tool_calling` is declared.
    pub programmatic: bool,
    /// `openai.apply_patch` is declared.
    pub apply_patch: bool,
    /// `openai.local_shell` is declared.
    pub local_shell: bool,
    /// `openai.shell` is declared.
    pub shell: bool,
    /// `openai.computer` is declared.
    pub computer: bool,
    /// Names of `openai.custom` tools (provider-side names).
    pub custom_tool_names: HashSet<String>,
}

impl ProviderToolSet {
    /// Whether `provider_name` is a tool whose calls are stored by the API.
    #[must_use]
    pub fn is_provider_defined(&self, provider_name: &str) -> bool {
        (self.local_shell && provider_name == "local_shell")
            || (self.shell && provider_name == "shell")
            || (self.apply_patch && provider_name == "apply_patch")
            || (self.computer && provider_name == "computer")
            || self.custom_tool_names.contains(provider_name)
    }
}

/// Settings that drive the conversion.
#[derive(Debug, Clone)]
pub struct ConversionContext<'a> {
    /// How system messages are sent.
    pub system_message_mode: SystemMessageMode,
    /// Whether the response is stored (`item_reference` allowed).
    pub store: bool,
    /// A conversation id is set.
    pub has_conversation: bool,
    /// A previous response id is set.
    pub has_previous_response_id: bool,
    /// Tool name mapping.
    pub tool_name_mapping: &'a ToolNameMapping,
    /// Provider-defined tools in the request.
    pub provider_tools: &'a ProviderToolSet,
    /// Prefixes identifying file ids in text file data.
    pub file_id_prefixes: &'a [String],
    /// Add `type: "message"` to message items.
    pub explicit_message_item_type: bool,
    /// Key of the provider options.
    pub provider_options_key: &'a str,
    /// Send unsupported file media types unchanged.
    pub pass_through_unsupported_files: bool,
}

/// Converted input.
#[derive(Debug, Clone, Default)]
pub struct ConvertedInput {
    /// Input items.
    pub input: Vec<JsonValue>,
    /// Warnings produced during conversion.
    pub warnings: Vec<Warning>,
}

/// Parses the part-level provider options.
pub(crate) fn part_options(
    options: Option<&ProviderOptions>,
    key: &str,
) -> Result<PartOptions, ProviderError> {
    let Some(options) = options else {
        return Ok(PartOptions::default());
    };
    Ok(parse_provider_options::<PartOptions>(key, options)?.unwrap_or_default())
}

/// Converts the prompt to input items.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for unsupported file
/// parts and [`ProviderError::InvalidArgument`] for invalid part options.
pub fn convert_prompt(
    prompt: &[PromptMessage],
    ctx: &ConversionContext<'_>,
) -> Result<ConvertedInput, ProviderError> {
    convert_prompt_for_provider(prompt, ctx, "openai")
}

pub(crate) fn convert_prompt_for_provider(
    prompt: &[PromptMessage],
    ctx: &ConversionContext<'_>,
    provider_name: &str,
) -> Result<ConvertedInput, ProviderError> {
    super::replay_advanced::validate_program_denials(prompt, ctx.provider_options_key)?;
    let mut out = ConvertedInput::default();
    for message in prompt {
        match message {
            PromptMessage::System { content, .. } => {
                let role = match ctx.system_message_mode {
                    SystemMessageMode::System => "system",
                    SystemMessageMode::Developer => "developer",
                    SystemMessageMode::Remove => {
                        out.warnings
                            .push(Warning::other("system messages are removed for this model"));
                        continue;
                    }
                };
                out.input
                    .push(message_item(ctx, json!({"role": role, "content": content})));
            }
            PromptMessage::User { content, .. } => {
                let mut parts = Vec::with_capacity(content.len());
                for (index, part) in content.iter().enumerate() {
                    parts.push(convert_user_part(part, index, ctx, provider_name)?);
                }
                out.input
                    .push(message_item(ctx, json!({"role": "user", "content": parts})));
            }
            PromptMessage::Assistant { content, .. } => {
                convert_assistant_message(content, ctx, &mut out)?;
            }
            PromptMessage::Tool { content, .. } => {
                convert_tool_message(content, ctx, &mut out)?;
            }
            #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
            _ => {}
        }
    }
    super::parallel::regroup(prompt, ctx, &mut out);
    Ok(out)
}

fn message_item(ctx: &ConversionContext<'_>, mut item: JsonValue) -> JsonValue {
    if ctx.explicit_message_item_type
        && let Some(object) = item.as_object_mut()
    {
        object.insert("type".to_owned(), JsonValue::from("message"));
    }
    item
}

fn convert_user_part(
    part: &UserPromptPart,
    index: usize,
    ctx: &ConversionContext<'_>,
    provider_name: &str,
) -> Result<JsonValue, ProviderError> {
    match part {
        UserPromptPart::Text(text) => Ok(json!({"type": "input_text", "text": text.text})),
        UserPromptPart::File(file) => convert_user_file(file, index, ctx, provider_name),
        #[allow(unreachable_patterns, reason = "UserPromptPart is non-exhaustive")]
        _ => Err(UnsupportedFunctionalityError::new("user prompt part type").into()),
    }
}

/// Media type used for wildcard image types.
fn image_media_type(media_type: &MediaType) -> String {
    if media_type.is_full() {
        media_type.as_str().to_owned()
    } else {
        "image/jpeg".to_owned()
    }
}

/// `data:<media>;base64,<payload>`.
pub(crate) fn data_url(media_type: &str, data: &FileData) -> Option<String> {
    data.to_base64()
        .map(|base64| format!("data:{media_type};base64,{base64}"))
}

fn file_id_from_text(text: &str, prefixes: &[String]) -> Option<String> {
    prefixes
        .iter()
        .any(|prefix| text.starts_with(prefix.as_str()))
        .then(|| text.to_owned())
}

fn convert_user_file(
    file: &FilePart,
    index: usize,
    ctx: &ConversionContext<'_>,
    provider_name: &str,
) -> Result<JsonValue, ProviderError> {
    let options = part_options(file.provider_options.as_ref(), ctx.provider_options_key)?;
    let is_image = file.media_type.top_level() == "image";
    let detail = options.image_detail.as_deref();
    let with_detail = |mut item: JsonValue| {
        if let Some(detail) = detail
            && let Some(object) = item.as_object_mut()
        {
            object.insert("detail".to_owned(), JsonValue::from(detail));
        }
        item
    };
    let file_id = match &file.data {
        FileData::Reference { reference } => {
            Some(resolve_provider_reference(reference, provider_name)?.to_owned())
        }
        FileData::Text { text } => Some(
            file_id_from_text(text, ctx.file_id_prefixes)
                .ok_or_else(|| UnsupportedFunctionalityError::new("text file parts"))?,
        ),
        _ => None,
    };
    if let Some(file_id) = file_id {
        return Ok(if is_image {
            with_detail(json!({"type": "input_image", "file_id": file_id}))
        } else {
            json!({"type": "input_file", "file_id": file_id})
        });
    }
    if is_image {
        let media_type = image_media_type(&file.media_type);
        let url = match &file.data {
            FileData::Url { url } => url.to_string(),
            data => data_url(&media_type, data)
                .ok_or_else(|| UnsupportedFunctionalityError::new("image file part data"))?,
        };
        return Ok(with_detail(
            json!({"type": "input_image", "image_url": url}),
        ));
    }
    match &file.data {
        FileData::Url { url } => Ok(json!({"type": "input_file", "file_url": url.to_string()})),
        data => {
            let media_type = file.media_type.as_str();
            if media_type != "application/pdf" && !ctx.pass_through_unsupported_files {
                return Err(UnsupportedFunctionalityError::new(format!(
                    "file part media type {media_type}"
                ))
                .into());
            }
            let file_data = data_url(media_type, data)
                .ok_or_else(|| UnsupportedFunctionalityError::new("file part data"))?;
            let filename = file
                .filename
                .clone()
                .unwrap_or_else(|| format!("part-{index}.pdf"));
            Ok(json!({"type": "input_file", "filename": filename, "file_data": file_data}))
        }
    }
}

fn convert_assistant_message(
    content: &[AssistantPromptPart],
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<(), ProviderError> {
    let key = ctx.provider_options_key;
    let mut reasoning_items: HashMap<String, usize> = HashMap::new();
    for part in content {
        match part {
            AssistantPromptPart::Text(text) => {
                let options = part_options(text.provider_options.as_ref(), key)?;
                let item_id = options.item_id.as_deref();
                if ctx.has_conversation && item_id.is_some() {
                    continue;
                }
                if ctx.store
                    && let Some(id) = item_id
                {
                    out.input.push(item_reference(id));
                    continue;
                }
                let mut item = json!({
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text.text}],
                });
                if let Some(object) = item.as_object_mut() {
                    if let Some(id) = item_id {
                        object.insert("id".to_owned(), JsonValue::from(id));
                    }
                    if let Some(phase) = &options.phase {
                        object.insert("phase".to_owned(), JsonValue::from(phase.as_str()));
                    }
                }
                out.input.push(message_item(ctx, item));
            }
            AssistantPromptPart::ToolCall(call) => convert_assistant_tool_call(call, ctx, out)?,
            AssistantPromptPart::ToolResult(result) => {
                if matches!(result.output, ToolResultOutput::ExecutionDenied { .. })
                    || is_denied_json(&result.output)
                    || ctx.has_conversation
                {
                    continue;
                }
                if super::replay_advanced::assistant_result(result, ctx, out)? {
                    continue;
                }
                if ctx.store {
                    let options = part_options(result.provider_options.as_ref(), key)?;
                    let id = options
                        .item_id
                        .unwrap_or_else(|| result.tool_call_id.as_str().to_owned());
                    out.input.push(item_reference(&id));
                } else {
                    out.warnings.push(Warning::other(format!(
                        "Results for OpenAI tool {} are not sent to the API when store is false",
                        result.tool_name
                    )));
                }
            }
            AssistantPromptPart::Reasoning(reasoning) => {
                let options = part_options(reasoning.provider_options.as_ref(), key)?;
                convert_reasoning(&reasoning.text, &options, ctx, out, &mut reasoning_items);
            }
            AssistantPromptPart::Custom(custom) => {
                if custom.kind.as_str() == "openai.compaction" {
                    let options = part_options(custom.provider_options.as_ref(), key)?;
                    let mut item = json!({"type": "compaction"});
                    if let Some(object) = item.as_object_mut() {
                        if let Some(id) = &options.item_id {
                            object.insert("id".to_owned(), JsonValue::from(id.as_str()));
                        }
                        object.insert(
                            "encrypted_content".to_owned(),
                            options
                                .encrypted_content
                                .clone()
                                .map_or(JsonValue::Null, JsonValue::from),
                        );
                    }
                    out.input.push(item);
                } else {
                    out.warnings.push(Warning::unsupported(format!(
                        "custom assistant part kind {}",
                        custom.kind
                    )));
                }
            }
            AssistantPromptPart::File(_) => {
                out.warnings
                    .push(Warning::unsupported("assistant file parts"));
            }
            AssistantPromptPart::ReasoningFile(_) => {
                out.warnings
                    .push(Warning::unsupported("assistant reasoning file parts"));
            }
            #[allow(unreachable_patterns, reason = "AssistantPromptPart is non-exhaustive")]
            _ => out
                .warnings
                .push(Warning::unsupported("assistant prompt part type")),
        }
    }
    Ok(())
}

fn is_denied_json(output: &ToolResultOutput) -> bool {
    matches!(output, ToolResultOutput::Json { value, .. }
        if value.get("type").and_then(JsonValue::as_str) == Some("execution-denied"))
}

pub(crate) fn item_reference(id: &str) -> JsonValue {
    json!({"type": "item_reference", "id": id})
}

fn convert_assistant_tool_call(
    call: &ToolCallPart,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
) -> Result<(), ProviderError> {
    let options = part_options(call.provider_options.as_ref(), ctx.provider_options_key)?;
    let item_id = options.item_id.as_deref();
    if super::replay_advanced::assistant_call(call, &options, ctx, out)? {
        return Ok(());
    }
    if call.provider_executed {
        if ctx.store
            && let Some(id) = item_id
        {
            out.input.push(item_reference(id));
        }
        return Ok(());
    }
    if ctx.has_conversation && item_id.is_some() {
        return Ok(());
    }
    let provider_name = ctx
        .tool_name_mapping
        .to_provider_tool_name(call.tool_name.as_str());
    let provider_defined = ctx.provider_tools.is_provider_defined(provider_name);
    if provider_defined {
        if let Some(id) = item_id
            && ctx.store
        {
            if !ctx.has_previous_response_id {
                out.input.push(item_reference(id));
            }
            return Ok(());
        }
        if ctx.provider_tools.apply_patch && provider_name == "apply_patch" {
            let call_id = call
                .input
                .get("callId")
                .and_then(JsonValue::as_str)
                .unwrap_or(call.tool_call_id.as_str());
            let mut item = json!({
                "type": "apply_patch_call",
                "call_id": call_id,
                "status": "completed",
                "operation": call.input.get("operation").cloned().unwrap_or(JsonValue::Null),
            });
            if let Some(id) = item_id
                && let Some(object) = item.as_object_mut()
            {
                object.insert("id".to_owned(), JsonValue::from(id));
            }
            out.input.push(item);
            return Ok(());
        }
        if ctx.provider_tools.custom_tool_names.contains(provider_name) {
            let input = match &call.input {
                JsonValue::String(text) => text.clone(),
                other => other.to_string(),
            };
            out.input.push(json!({
                "type": "custom_tool_call",
                "call_id": call.tool_call_id,
                "name": provider_name,
                "input": input,
            }));
            return Ok(());
        }
        out.warnings.push(Warning::other(format!(
            "Calls of OpenAI tool {provider_name} are not sent to the API when store is false"
        )));
        return Ok(());
    }
    let mut item = json!({
        "type": "function_call",
        "call_id": call.tool_call_id,
        "name": provider_name,
        "arguments": call.input.to_string(),
    });
    if let Some(object) = item.as_object_mut()
        && let Some(id) = item_id
    {
        object.insert("id".to_owned(), JsonValue::from(id));
    }
    super::replay_advanced::add_function_options(&mut item, &options);
    out.input.push(item);
    Ok(())
}

fn convert_reasoning(
    text: &str,
    options: &PartOptions,
    ctx: &ConversionContext<'_>,
    out: &mut ConvertedInput,
    reasoning_items: &mut HashMap<String, usize>,
) {
    let summary = |text: &str| -> Vec<JsonValue> {
        if text.is_empty() {
            Vec::new()
        } else {
            vec![json!({"type": "summary_text", "text": text})]
        }
    };
    match &options.item_id {
        Some(id) => {
            if ctx.has_conversation || ctx.has_previous_response_id {
                return;
            }
            if ctx.store {
                if !reasoning_items.contains_key(id) {
                    out.input.push(item_reference(id));
                    reasoning_items.insert(id.clone(), out.input.len() - 1);
                }
                return;
            }
            if let Some(index) = reasoning_items.get(id).copied()
                && let Some(existing) = out.input.get_mut(index)
                && let Some(list) = existing
                    .get_mut("summary")
                    .and_then(JsonValue::as_array_mut)
            {
                list.extend(summary(text));
                if let Some(encrypted) = &options.reasoning_encrypted_content
                    && let Some(object) = existing.as_object_mut()
                {
                    object.insert(
                        "encrypted_content".to_owned(),
                        JsonValue::from(encrypted.as_str()),
                    );
                }
                return;
            }
            let mut item = JsonObject::new();
            item.insert("type".to_owned(), JsonValue::from("reasoning"));
            item.insert("id".to_owned(), JsonValue::from(id.as_str()));
            item.insert(
                "encrypted_content".to_owned(),
                options
                    .reasoning_encrypted_content
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            item.insert("summary".to_owned(), JsonValue::Array(summary(text)));
            out.input.push(JsonValue::Object(item));
            reasoning_items.insert(id.clone(), out.input.len() - 1);
        }
        None => match &options.reasoning_encrypted_content {
            Some(encrypted) => out.input.push(json!({
                "type": "reasoning",
                "encrypted_content": encrypted,
                "summary": summary(text),
            })),
            None => out.warnings.push(Warning::other(format!(
                "Non-OpenAI reasoning parts are not supported. Skipping reasoning part: {text}."
            ))),
        },
    }
}
