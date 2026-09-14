//! Conversion of the specification prompt to Chat Completions messages.
//!
//! Objects under the `openaiCompatible` key of a message's or part's
//! `provider_options` are spread into the wire object.

use ferrin_provider_util::media_type::resolve_full_media_type;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;

use crate::options_key::shared_extra_fields;

/// Text sent for a denied tool call without a reason.
pub const EXECUTION_DENIED_MESSAGE: &str = "Tool call execution denied.";

/// Converted messages.
#[derive(Debug, Clone, Default)]
pub struct ConvertedMessages {
    /// `messages` array.
    pub messages: Vec<JsonValue>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts the prompt; `metadata_key` is the provider options key under
/// which tool call parts carry `thoughtSignature`.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for unsupported file
/// parts.
pub fn convert_prompt(
    prompt: &[PromptMessage],
    metadata_key: &str,
) -> Result<ConvertedMessages, ProviderError> {
    let mut out = ConvertedMessages::default();
    for message in prompt {
        match message {
            PromptMessage::System {
                content,
                provider_options,
            } => out.messages.push(with_extra(
                json!({"role": "system", "content": content}),
                provider_options.as_ref(),
            )),
            PromptMessage::User {
                content,
                provider_options,
            } => out.messages.push(convert_user(
                content,
                provider_options.as_ref(),
                &mut out.warnings,
            )?),
            PromptMessage::Assistant {
                content,
                provider_options,
            } => out.messages.push(convert_assistant(
                content,
                provider_options.as_ref(),
                metadata_key,
                &mut out.warnings,
            )),
            PromptMessage::Tool { content, .. } => {
                for part in content {
                    match part {
                        ToolPromptPart::ToolResult(result) => {
                            let content = match &result.output {
                                ToolResultOutput::Text { value, .. }
                                | ToolResultOutput::ErrorText { value, .. } => value.clone(),
                                ToolResultOutput::ExecutionDenied { reason, .. } => reason
                                    .clone()
                                    .unwrap_or_else(|| EXECUTION_DENIED_MESSAGE.to_owned()),
                                ToolResultOutput::Json { value, .. }
                                | ToolResultOutput::ErrorJson { value, .. } => value.to_string(),
                                ToolResultOutput::Content { value } => {
                                    serde_json::to_string(value).map_err(ProviderError::other)?
                                }
                                #[allow(
                                    unreachable_patterns,
                                    reason = "ToolResultOutput is non-exhaustive"
                                )]
                                _ => {
                                    return Err(UnsupportedFunctionalityError::new(
                                        "tool result output type",
                                    )
                                    .into());
                                }
                            };
                            out.messages.push(with_extra(
                                json!({
                                    "role": "tool",
                                    "tool_call_id": result.tool_call_id,
                                    "content": content,
                                }),
                                result.provider_options.as_ref(),
                            ));
                        }
                        ToolPromptPart::ToolApprovalResponse(_) => {}
                        #[allow(unreachable_patterns, reason = "ToolPromptPart is non-exhaustive")]
                        _ => out
                            .warnings
                            .push(Warning::unsupported("tool prompt part type")),
                    }
                }
            }
            #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
            _ => out
                .warnings
                .push(Warning::unsupported("prompt message role")),
        }
    }
    Ok(out)
}

fn with_extra(mut value: JsonValue, provider_options: Option<&ProviderOptions>) -> JsonValue {
    let extra = shared_extra_fields(provider_options);
    if !extra.is_empty()
        && let Some(object) = value.as_object_mut()
    {
        object.extend(extra);
    }
    value
}

fn convert_user(
    content: &[UserPromptPart],
    provider_options: Option<&ProviderOptions>,
    warnings: &mut Vec<Warning>,
) -> Result<JsonValue, ProviderError> {
    if let [UserPromptPart::Text(text)] = content {
        return Ok(with_extra(
            with_extra(
                json!({"role": "user", "content": text.text}),
                text.provider_options.as_ref(),
            ),
            provider_options,
        ));
    }
    let mut parts = Vec::with_capacity(content.len());
    for part in content {
        match part {
            UserPromptPart::Text(text) => parts.push(with_extra(
                json!({"type": "text", "text": text.text}),
                text.provider_options.as_ref(),
            )),
            UserPromptPart::File(file) => parts.push(with_extra(
                convert_file(file)?,
                file.provider_options.as_ref(),
            )),
            #[allow(unreachable_patterns, reason = "UserPromptPart is non-exhaustive")]
            _ => warnings.push(Warning::unsupported("user prompt part type")),
        }
    }
    Ok(with_extra(
        json!({"role": "user", "content": parts}),
        provider_options,
    ))
}

fn convert_file(file: &FilePart) -> Result<JsonValue, ProviderError> {
    let unsupported =
        |what: String| -> ProviderError { UnsupportedFunctionalityError::new(what).into() };
    let (url, bytes) = match &file.data {
        FileData::Reference { .. } => {
            return Err(unsupported(
                "file parts with provider references".to_owned(),
            ));
        }
        FileData::Text { .. } => return Err(unsupported("text file parts".to_owned())),
        FileData::Url { url } => (Some(url), None),
        FileData::Bytes { data } => (None, Some(data)),
        #[allow(unreachable_patterns, reason = "FileData is non-exhaustive")]
        _ => return Err(unsupported("file data type".to_owned())),
    };
    let top_level = file.media_type.top_level();
    let data_url = |full: &str| -> String {
        format!(
            "data:{full};base64,{}",
            file.data.to_base64().unwrap_or_default()
        )
    };
    match top_level.as_str() {
        "image" | "video" => {
            let value = match url {
                Some(url) => url.to_string(),
                None => data_url(
                    resolve_full_media_type(&file.media_type, bytes.map(AsRef::as_ref))?.as_str(),
                ),
            };
            Ok(if top_level == "image" {
                json!({"type": "image_url", "image_url": {"url": value}})
            } else {
                json!({"type": "video_url", "video_url": {"url": value}})
            })
        }
        "audio" => {
            let Some(bytes) = bytes else {
                return Err(unsupported("audio file parts with URLs".to_owned()));
            };
            let full = resolve_full_media_type(&file.media_type, Some(bytes.as_ref()))?;
            let format = match full.as_str() {
                "audio/wav" => "wav",
                "audio/mp3" | "audio/mpeg" => "mp3",
                other => return Err(unsupported(format!("audio media type {other}"))),
            };
            Ok(json!({
                "type": "input_audio",
                "input_audio": {"data": file.data.to_base64().unwrap_or_default(), "format": format},
            }))
        }
        "application" => {
            let Some(bytes) = bytes else {
                return Err(unsupported("PDF file parts with URLs".to_owned()));
            };
            let full = resolve_full_media_type(&file.media_type, Some(bytes.as_ref()))?;
            if full.as_str() != "application/pdf" {
                return Err(unsupported(format!("file part media type {full}")));
            }
            Ok(json!({
                "type": "file",
                "file": {
                    "filename": file.filename.clone().unwrap_or_else(|| "document.pdf".to_owned()),
                    "file_data": data_url("application/pdf"),
                },
            }))
        }
        "text" => {
            let text = match (url, bytes) {
                (Some(url), _) => url.to_string(),
                (None, Some(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
                (None, None) => String::new(),
            };
            Ok(json!({"type": "text", "text": text}))
        }
        _ => Err(unsupported(format!(
            "file part media type {}",
            file.media_type
        ))),
    }
}

fn convert_assistant(
    content: &[AssistantPromptPart],
    provider_options: Option<&ProviderOptions>,
    metadata_key: &str,
    warnings: &mut Vec<Warning>,
) -> JsonValue {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for part in content {
        match part {
            AssistantPromptPart::Text(t) => text.push_str(&t.text),
            AssistantPromptPart::Reasoning(r) => reasoning.push_str(&r.text),
            AssistantPromptPart::ToolCall(call) => {
                let mut value = with_extra(
                    json!({
                        "id": call.tool_call_id,
                        "type": "function",
                        "function": {
                            "name": call.tool_name,
                            "arguments": call.input.to_string(),
                        },
                    }),
                    call.provider_options.as_ref(),
                );
                if let Some(signature) =
                    thought_signature(call.provider_options.as_ref(), metadata_key)
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert(
                        "extra_content".to_owned(),
                        json!({"google": {"thought_signature": signature}}),
                    );
                }
                tool_calls.push(value);
            }
            AssistantPromptPart::ToolResult(_) => {}
            AssistantPromptPart::File(_) => {
                warnings.push(Warning::unsupported("assistant file parts"));
            }
            #[allow(unreachable_patterns, reason = "AssistantPromptPart is non-exhaustive")]
            _ => warnings.push(Warning::unsupported("assistant prompt part type")),
        }
    }
    let mut message = JsonObject::new();
    message.insert("role".to_owned(), JsonValue::from("assistant"));
    message.insert(
        "content".to_owned(),
        if !tool_calls.is_empty() && text.is_empty() {
            JsonValue::Null
        } else {
            JsonValue::from(text)
        },
    );
    if !reasoning.is_empty() {
        message.insert("reasoning_content".to_owned(), JsonValue::from(reasoning));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_owned(), JsonValue::Array(tool_calls));
    }
    with_extra(JsonValue::Object(message), provider_options)
}

/// `thoughtSignature` under `metadata_key`, falling back to the `google`
/// key.
fn thought_signature(options: Option<&ProviderOptions>, metadata_key: &str) -> Option<String> {
    let options = options?;
    [metadata_key, "google"].iter().find_map(|key| {
        options
            .get(*key)?
            .get("thoughtSignature")
            .map(|value| match value {
                JsonValue::String(text) => text.clone(),
                other => other.to_string(),
            })
    })
}
