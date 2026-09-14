//! Conversion of the specification prompt to Chat Completions messages.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_provider_util::media_type::resolve_full_media_type;
use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_spec::FileData;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;

use crate::capabilities::SystemMessageMode;
use crate::responses::convert_prompt::part_options;
use crate::responses::convert_tool_results::EXECUTION_DENIED_MESSAGE;

/// Converted messages.
#[derive(Debug, Clone, Default)]
pub struct ConvertedMessages {
    /// `messages` array.
    pub messages: Vec<JsonValue>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts the prompt.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for unsupported file
/// parts.
pub fn convert_prompt(
    prompt: &[PromptMessage],
    system_message_mode: SystemMessageMode,
    provider_options_key: &str,
) -> Result<ConvertedMessages, ProviderError> {
    let mut out = ConvertedMessages::default();
    for message in prompt {
        match message {
            PromptMessage::System { content, .. } => match system_message_mode {
                SystemMessageMode::System => out
                    .messages
                    .push(json!({"role": "system", "content": content})),
                SystemMessageMode::Developer => out
                    .messages
                    .push(json!({"role": "developer", "content": content})),
                SystemMessageMode::Remove => out
                    .warnings
                    .push(Warning::other("system messages are removed for this model")),
            },
            PromptMessage::User { content, .. } => {
                if let [UserPromptPart::Text(text)] = content.as_slice() {
                    out.messages
                        .push(json!({"role": "user", "content": text.text}));
                    continue;
                }
                let mut parts = Vec::with_capacity(content.len());
                for (index, part) in content.iter().enumerate() {
                    parts.push(match part {
                        UserPromptPart::Text(text) => json!({"type": "text", "text": text.text}),
                        UserPromptPart::File(file) => {
                            convert_file(file, index, provider_options_key)?
                        }
                        #[allow(unreachable_patterns, reason = "UserPromptPart is non-exhaustive")]
                        _ => {
                            return Err(UnsupportedFunctionalityError::new(
                                "user prompt part type",
                            )
                            .into());
                        }
                    });
                }
                out.messages.push(json!({"role": "user", "content": parts}));
            }
            PromptMessage::Assistant { content, .. } => {
                let mut text = String::new();
                let mut tool_calls = Vec::new();
                for part in content {
                    match part {
                        AssistantPromptPart::Text(t) => text.push_str(&t.text),
                        AssistantPromptPart::ToolCall(call) => tool_calls.push(json!({
                            "id": call.tool_call_id,
                            "type": "function",
                            "function": {
                                "name": call.tool_name,
                                "arguments": call.input.to_string(),
                            },
                        })),
                        AssistantPromptPart::Reasoning(_) | AssistantPromptPart::ToolResult(_) => {}
                        AssistantPromptPart::File(_) => {
                            out.warnings
                                .push(Warning::unsupported("assistant file parts"));
                        }
                        _ => {
                            out.warnings
                                .push(Warning::unsupported("assistant prompt part type"));
                        }
                    }
                }
                let mut message = json!({"role": "assistant"});
                if let Some(object) = message.as_object_mut() {
                    object.insert(
                        "content".to_owned(),
                        if text.is_empty() && !tool_calls.is_empty() {
                            JsonValue::Null
                        } else {
                            JsonValue::from(text)
                        },
                    );
                    if !tool_calls.is_empty() {
                        object.insert("tool_calls".to_owned(), JsonValue::Array(tool_calls));
                    }
                }
                out.messages.push(message);
            }
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
                                    let items: Vec<JsonValue> = value
                                        .iter()
                                        .filter_map(|item| match item {
                                            ToolResultContentPart::Text { text, .. } => {
                                                Some(json!({"type": "text", "text": text}))
                                            }
                                            _ => {
                                                out.warnings.push(Warning::other(
                                                    "unsupported tool result content part type for Chat Completions",
                                                ));
                                                None
                                            }
                                        })
                                        .collect();
                                    JsonValue::Array(items).to_string()
                                }
                                _ => {
                                    return Err(UnsupportedFunctionalityError::new(
                                        "tool result output type",
                                    )
                                    .into());
                                }
                            };
                            out.messages.push(json!({
                                "role": "tool",
                                "tool_call_id": result.tool_call_id,
                                "content": content,
                            }));
                        }
                        ToolPromptPart::ToolApprovalResponse(_) => {}
                        _ => out
                            .warnings
                            .push(Warning::unsupported("tool prompt part type")),
                    }
                }
            }
            #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
            _ => {}
        }
    }
    Ok(out)
}

fn convert_file(file: &FilePart, index: usize, key: &str) -> Result<JsonValue, ProviderError> {
    let options = part_options(file.provider_options.as_ref(), key)?;
    match &file.data {
        FileData::Reference { reference } => Ok(json!({
            "type": "file",
            "file": {"file_id": resolve_provider_reference(reference, "openai")?},
        })),
        FileData::Text { .. } => Err(UnsupportedFunctionalityError::new("text file parts").into()),
        FileData::Url { url } => {
            let top_level = file.media_type.top_level();
            match top_level.as_str() {
                "image" => {
                    let mut image_url = json!({"url": url.to_string()});
                    if let Some(detail) = &options.image_detail
                        && let Some(object) = image_url.as_object_mut()
                    {
                        object.insert("detail".to_owned(), JsonValue::from(detail.as_str()));
                    }
                    Ok(json!({"type": "image_url", "image_url": image_url}))
                }
                "audio" => {
                    Err(UnsupportedFunctionalityError::new("audio file parts with URLs").into())
                }
                _ => {
                    let full = resolve_full_media_type(&file.media_type, None).ok();
                    if full
                        .as_ref()
                        .is_none_or(|m| m.as_str() != "application/pdf")
                    {
                        return Err(UnsupportedFunctionalityError::new(format!(
                            "file part media type {}",
                            file.media_type
                        ))
                        .into());
                    }
                    Err(UnsupportedFunctionalityError::new("PDF file parts with URLs").into())
                }
            }
        }
        FileData::Bytes { data } => {
            let top_level = file.media_type.top_level();
            let full = resolve_full_media_type(&file.media_type, Some(data))?;
            let base64 = file.data.to_base64().unwrap_or_default();
            match top_level.as_str() {
                "image" => {
                    let mut image_url = json!({"url": format!("data:{full};base64,{base64}")});
                    if let Some(detail) = &options.image_detail
                        && let Some(object) = image_url.as_object_mut()
                    {
                        object.insert("detail".to_owned(), JsonValue::from(detail.as_str()));
                    }
                    Ok(json!({"type": "image_url", "image_url": image_url}))
                }
                "audio" => {
                    let format = match full.as_str() {
                        "audio/wav" => "wav",
                        "audio/mp3" | "audio/mpeg" => "mp3",
                        other => {
                            return Err(UnsupportedFunctionalityError::new(format!(
                                "audio content parts with media type {other}"
                            ))
                            .into());
                        }
                    };
                    Ok(
                        json!({"type": "input_audio", "input_audio": {"data": base64, "format": format}}),
                    )
                }
                _ => {
                    if full.as_str() != "application/pdf" {
                        return Err(UnsupportedFunctionalityError::new(format!(
                            "file part media type {full}"
                        ))
                        .into());
                    }
                    Ok(json!({
                        "type": "file",
                        "file": {
                            "filename": file.filename.clone().unwrap_or_else(|| format!("part-{index}.pdf")),
                            "file_data": format!("data:application/pdf;base64,{base64}"),
                        },
                    }))
                }
            }
        }
        #[allow(unreachable_patterns, reason = "FileData is non-exhaustive")]
        _ => Err(UnsupportedFunctionalityError::new("file data type").into()),
    }
}
