//! User-role content: text, files and client tool results.

use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_spec::FileData;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderOptions;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;

use super::BETA_FILES_API;
use super::BETA_PDFS;
use super::Converter;
use super::with_cache_control;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::options::FilePartOptions;
use crate::options::read_options;

/// Default text of a denied tool execution.
pub(crate) const EXECUTION_DENIED_TEXT: &str = "Tool call execution denied.";

pub(super) fn convert_user_group(
    converter: &mut Converter<'_>,
    group: &[&PromptMessage],
) -> Result<Vec<JsonValue>, ProviderError> {
    let mut blocks = Vec::new();
    for message in group {
        match message {
            PromptMessage::User {
                content,
                provider_options,
            } => {
                let count = content.len();
                for (index, part) in content.iter().enumerate() {
                    let is_last = index + 1 == count;
                    match part {
                        UserPromptPart::Text(text) => {
                            let cache_control = converter.cache_control(
                                text.provider_options.as_ref(),
                                provider_options.as_ref(),
                                is_last,
                                "text",
                                true,
                            );
                            blocks.push(with_cache_control(
                                json!({"type": "text", "text": text.text}),
                                cache_control,
                            ));
                        }
                        UserPromptPart::File(file) => {
                            let cache_control = converter.cache_control(
                                file.provider_options.as_ref(),
                                provider_options.as_ref(),
                                is_last,
                                "file",
                                true,
                            );
                            blocks.push(with_cache_control(
                                convert_file(converter, file)?,
                                cache_control,
                            ));
                        }
                        #[allow(unreachable_patterns, reason = "UserPromptPart is non-exhaustive")]
                        _ => converter.warn_unsupported("user prompt part"),
                    }
                }
            }
            PromptMessage::Tool {
                content,
                provider_options,
            } => {
                let count = content.len();
                for (index, part) in content.iter().enumerate() {
                    let is_last = index + 1 == count;
                    match part {
                        ToolPromptPart::ToolResult(result) => {
                            blocks.push(convert_tool_result(
                                converter,
                                result,
                                provider_options.as_ref(),
                                is_last,
                            )?);
                        }
                        ToolPromptPart::ToolApprovalResponse(_) => {
                            converter.warn_unsupported("tool approval responses");
                        }
                        #[allow(unreachable_patterns, reason = "ToolPromptPart is non-exhaustive")]
                        _ => converter.warn_unsupported("tool prompt part"),
                    }
                }
            }
            _ => {}
        }
    }
    Ok(blocks)
}

fn file_options(converter: &Converter<'_>, file: &FilePart) -> FilePartOptions {
    read_options(converter.options(file.provider_options.as_ref())).unwrap_or_default()
}

fn document_fields(
    block: &mut JsonValue,
    file: &FilePart,
    options: &FilePartOptions,
    default_title: bool,
) {
    let Some(object) = block.as_object_mut() else {
        return;
    };
    let title = options.title.clone().or_else(|| {
        if default_title {
            file.filename.clone()
        } else {
            None
        }
    });
    if let Some(title) = title {
        object.insert("title".to_owned(), JsonValue::from(title));
    }
    if let Some(context) = &options.context {
        object.insert("context".to_owned(), JsonValue::from(context.clone()));
    }
    if let Some(citations) = &options.citations {
        object.insert(
            "citations".to_owned(),
            json!({"enabled": citations.enabled}),
        );
    }
}

fn file_id<'a>(converter: &Converter<'_>, file: &'a FilePart) -> Result<&'a str, ProviderError> {
    let Some(reference) = file.data.as_reference() else {
        return Err(UnsupportedFunctionalityError::new("file reference").into());
    };
    match resolve_provider_reference(reference, converter.config.options_key()) {
        Ok(id) => Ok(id),
        Err(error) => match reference.get(CANONICAL_OPTIONS_KEY) {
            Some(id) if converter.config.options_key() != CANONICAL_OPTIONS_KEY => Ok(id),
            _ => Err(error.into()),
        },
    }
}

/// Converts a user file part into an `image`, `document` or
/// `container_upload` block.
pub(super) fn convert_file(
    converter: &mut Converter<'_>,
    file: &FilePart,
) -> Result<JsonValue, ProviderError> {
    let options = file_options(converter, file);
    let media_type = file.media_type.normalize();
    let is_image = media_type.top_level() == "image";
    if matches!(file.data, FileData::Reference { .. }) {
        converter.betas.insert(BETA_FILES_API.to_owned());
        let id = file_id(converter, file)?;
        if options.container_upload == Some(true) {
            return Ok(json!({"type": "container_upload", "file_id": id}));
        }
        if is_image {
            return Ok(json!({"type": "image", "source": {"type": "file", "file_id": id}}));
        }
        let mut block = json!({"type": "document", "source": {"type": "file", "file_id": id}});
        document_fields(&mut block, file, &options, true);
        return Ok(block);
    }
    if let FileData::Text { text } = &file.data {
        let mut block = json!({
            "type": "document",
            "source": {"type": "text", "media_type": "text/plain", "data": text},
        });
        document_fields(&mut block, file, &options, true);
        return Ok(block);
    }
    if is_image {
        let source = match &file.data {
            FileData::Url { url } => json!({"type": "url", "url": url.as_str()}),
            other => json!({
                "type": "base64",
                "media_type": media_type.as_str(),
                "data": other.to_base64().unwrap_or_default(),
            }),
        };
        return Ok(json!({"type": "image", "source": source}));
    }
    match media_type.as_str() {
        "application/pdf" => {
            converter.betas.insert(BETA_PDFS.to_owned());
            let source = match &file.data {
                FileData::Url { url } => json!({"type": "url", "url": url.as_str()}),
                other => json!({
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": other.to_base64().unwrap_or_default(),
                }),
            };
            let mut block = json!({"type": "document", "source": source});
            document_fields(&mut block, file, &options, true);
            Ok(block)
        }
        "text/plain" => {
            let source = match &file.data {
                FileData::Url { url } => json!({"type": "url", "url": url.as_str()}),
                FileData::Bytes { data } => json!({
                    "type": "text",
                    "media_type": "text/plain",
                    "data": String::from_utf8_lossy(data),
                }),
                _ => json!({"type": "text", "media_type": "text/plain", "data": ""}),
            };
            let mut block = json!({"type": "document", "source": source});
            document_fields(&mut block, file, &options, true);
            Ok(block)
        }
        other => Err(UnsupportedFunctionalityError::new(format!("media type: {other}")).into()),
    }
}

fn tool_result_content_parts(
    converter: &mut Converter<'_>,
    parts: &[ToolResultContentPart],
) -> Result<Vec<JsonValue>, ProviderError> {
    let mut blocks = Vec::new();
    for part in parts {
        match part {
            ToolResultContentPart::Text { text, .. } => {
                blocks.push(json!({"type": "text", "text": text}));
            }
            ToolResultContentPart::File {
                data,
                media_type,
                filename,
                provider_options,
            } => {
                let file = FilePart {
                    data: data.clone(),
                    media_type: media_type.clone(),
                    filename: filename.clone(),
                    provider_options: provider_options.clone(),
                };
                let media = MediaType::new(media_type.as_str()).normalize();
                if media.top_level() == "image"
                    || media.as_str() == "application/pdf"
                    || media.as_str() == "text/plain"
                    || matches!(data, FileData::Reference { .. } | FileData::Text { .. })
                {
                    blocks.push(convert_file(converter, &file)?);
                } else {
                    converter.warn_unsupported(format!(
                        "tool result content file media type: {}",
                        media.as_str()
                    ));
                }
            }
            ToolResultContentPart::Custom { provider_options } => {
                let options = converter.options(provider_options.as_ref());
                let tool_name = options
                    .and_then(|o| o.get("toolName").or_else(|| o.get("tool_name")))
                    .and_then(JsonValue::as_str);
                match tool_name {
                    Some(name) => {
                        blocks.push(json!({"type": "tool_reference", "tool_name": name}));
                    }
                    None => converter.warn_unsupported("custom tool result content part"),
                }
            }
            #[allow(
                unreachable_patterns,
                reason = "ToolResultContentPart is non-exhaustive"
            )]
            _ => converter.warn_unsupported("tool result content part"),
        }
    }
    Ok(blocks)
}

/// Converts a client tool result into a `tool_result` block.
pub(super) fn convert_tool_result(
    converter: &mut Converter<'_>,
    result: &ToolResultPart,
    message_options: Option<&ProviderOptions>,
    is_last: bool,
) -> Result<JsonValue, ProviderError> {
    let cache_control = converter.cache_control(
        result.provider_options.as_ref(),
        message_options,
        is_last,
        "tool result",
        true,
    );
    let (content, is_error) = match &result.output {
        ToolResultOutput::Text { value, .. } => (JsonValue::from(value.clone()), false),
        ToolResultOutput::Json { value, .. } => (JsonValue::from(value.to_string()), false),
        ToolResultOutput::ErrorText { value, .. } => (JsonValue::from(value.clone()), true),
        ToolResultOutput::ErrorJson { value, .. } => (JsonValue::from(value.to_string()), true),
        ToolResultOutput::ExecutionDenied { reason, .. } => (
            JsonValue::from(
                reason
                    .clone()
                    .unwrap_or_else(|| EXECUTION_DENIED_TEXT.to_owned()),
            ),
            true,
        ),
        ToolResultOutput::Content { value } => (
            JsonValue::Array(tool_result_content_parts(converter, value)?),
            false,
        ),
        #[allow(unreachable_patterns, reason = "ToolResultOutput is non-exhaustive")]
        _ => {
            converter.warn_unsupported("tool result output type");
            (JsonValue::from(""), false)
        }
    };
    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": result.tool_call_id.as_str(),
        "content": content,
    });
    if is_error && let Some(object) = block.as_object_mut() {
        object.insert("is_error".to_owned(), JsonValue::Bool(true));
    }
    Ok(with_cache_control(block, cache_control))
}
