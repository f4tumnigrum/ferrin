//! Conversion of application messages into the provider prompt.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use bytes::Bytes;
use ferrin_message::AssistantContent;
use ferrin_message::AssistantMessage;
use ferrin_message::AssistantPart;
use ferrin_message::FileSource;
use ferrin_message::Message;
use ferrin_message::ToolMessage;
use ferrin_message::ToolPart;
use ferrin_message::UserContent;
use ferrin_message::UserMessage;
use ferrin_message::UserPart;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_spec::FileData;
use ferrin_spec::MediaType;
use ferrin_spec::Prompt;
use ferrin_spec::PromptMessage;
use ferrin_spec::SupportedUrls;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::ReasoningFilePart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::ToolApprovalResponsePart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::Instructions;
use super::download::DefaultDownloader;
use super::download::DownloadFn;
use super::download::DownloadRequest;
use super::download::DownloadedFile;
use crate::error::Error;
use crate::middleware::builtin::merge_provider_options;

/// Successful URL downloads reused across steps of a single invocation.
pub(crate) type DownloadCache = Mutex<HashMap<String, DownloadedFile>>;

/// Inputs of a conversion.
pub(crate) struct ConvertContext<'a> {
    /// URL patterns the model fetches itself.
    pub(crate) supported_urls: &'a SupportedUrls,
    /// Download function; `None` uses the default downloader.
    pub(crate) download: Option<&'a dyn DownloadFn>,
    /// Invocation cache; batch conversions may omit it.
    pub(crate) cache: Option<&'a DownloadCache>,
    /// Cancellation for downloads.
    pub(crate) cancellation: &'a CancellationToken,
}

/// Converts `system` and `messages` into a provider prompt.
///
/// URLs the model cannot fetch are downloaded and inlined; images become
/// file parts with detected media types; approval requests and non
/// provider-executed approval responses are stripped.
pub(crate) async fn convert_to_prompt(
    system: Option<&Instructions>,
    messages: &[Message],
    ctx: ConvertContext<'_>,
) -> Result<Prompt, Error> {
    let downloaded = download_files(messages, &ctx).await?;
    let mut prompt: Prompt = Vec::with_capacity(messages.len() + 1);
    if let Some(system) = system {
        prompt.extend(
            system
                .as_messages()
                .iter()
                .map(|message| PromptMessage::System {
                    content: message.content.clone(),
                    provider_options: message.provider_options.clone(),
                }),
        );
    }
    for message in messages {
        let converted = match message {
            Message::System(system) => Some(PromptMessage::System {
                content: system.content.clone(),
                provider_options: system.provider_options.clone(),
            }),
            Message::User(user) => Some(convert_user(user, &downloaded).await?),
            Message::Assistant(assistant) => convert_assistant(assistant, message, &downloaded)?,
            Message::Tool(tool) => Some(convert_tool(tool, &downloaded)?),
            #[allow(unreachable_patterns, reason = "the message enum is non-exhaustive")]
            _ => {
                return Err(Error::MessageConversion {
                    message: "unsupported message role".to_owned(),
                    original_message: Box::new(message.clone()),
                });
            }
        };
        if let Some(converted) = converted {
            append_message(&mut prompt, converted);
        }
    }
    validate_tool_results(&prompt, messages)?;
    prompt.retain(
        |message| !matches!(message, PromptMessage::Tool { content, .. } if content.is_empty()),
    );
    Ok(prompt)
}

fn validate_tool_results(prompt: &Prompt, messages: &[Message]) -> Result<(), Error> {
    let approvals: HashMap<_, _> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant(AssistantMessage {
                content: AssistantContent::Parts(parts),
                ..
            }) => Some(parts),
            _ => None,
        })
        .flatten()
        .filter_map(|part| {
            part.as_tool_approval_request()
                .map(|request| (&request.approval_id, &request.tool_call_id))
        })
        .collect();
    let exempt: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool(tool) => Some(&tool.content),
            _ => None,
        })
        .flatten()
        .filter_map(|part| {
            part.as_tool_approval_response()
                .and_then(|response| approvals.get(&response.approval_id).copied())
        })
        .collect();
    let mut pending = Vec::new();
    for message in prompt {
        match message {
            PromptMessage::Assistant { content, .. } => {
                for part in content {
                    if let AssistantPromptPart::ToolCall(call) = part
                        && !call.provider_executed
                        && !pending.contains(&call.tool_call_id)
                    {
                        pending.push(call.tool_call_id.clone());
                    }
                }
            }
            PromptMessage::Tool { content, .. } => {
                for part in content {
                    if let ToolPromptPart::ToolResult(result) = part {
                        pending.retain(|id| id != &result.tool_call_id);
                    }
                }
            }
            PromptMessage::User { .. } | PromptMessage::System { .. } => {
                pending.retain(|id| !exempt.contains(&id));
                require_tool_results(&pending)?;
            }
            #[allow(unreachable_patterns, reason = "the message enum is non-exhaustive")]
            _ => {}
        }
    }
    pending.retain(|id| !exempt.contains(&id));
    require_tool_results(&pending)
}

fn require_tool_results(pending: &[ferrin_spec::ToolCallId]) -> Result<(), Error> {
    if pending.is_empty() {
        return Ok(());
    }
    Err(Error::invalid_prompt(format!(
        "missing tool results for calls: {}",
        pending
            .iter()
            .map(ferrin_spec::ToolCallId::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn append_message(prompt: &mut Prompt, message: PromptMessage) {
    if let PromptMessage::Tool {
        content,
        provider_options,
    } = message
    {
        if let Some(PromptMessage::Tool {
            content: previous,
            provider_options: previous_options,
        }) = prompt.last_mut()
        {
            if let (Some(part), Some(options)) = (previous.last_mut(), previous_options.take()) {
                let part_options = match part {
                    ToolPromptPart::ToolResult(part) => &mut part.provider_options,
                    ToolPromptPart::ToolApprovalResponse(part) => &mut part.provider_options,
                    #[allow(unreachable_patterns, reason = "the part enum is non-exhaustive")]
                    _ => return,
                };
                *part_options = Some(merge_provider_options(
                    &options,
                    part_options.take().unwrap_or_default(),
                ));
            }
            previous.extend(content);
            *previous_options = provider_options;
        } else {
            prompt.push(PromptMessage::Tool {
                content,
                provider_options,
            });
        }
    } else {
        prompt.push(message);
    }
}

async fn download_files(
    messages: &[Message],
    ctx: &ConvertContext<'_>,
) -> Result<HashMap<String, DownloadedFile>, Error> {
    let mut downloaded = ctx.cache.map_or_else(HashMap::new, |cache| {
        cache.lock().unwrap_or_else(PoisonError::into_inner).clone()
    });
    let mut requests: Vec<DownloadRequest> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut targets = Vec::new();
    for message in messages {
        let results: Vec<_> = match message {
            Message::Tool(tool) => tool
                .content
                .iter()
                .filter_map(ToolPart::as_tool_result)
                .collect(),
            Message::Assistant(AssistantMessage {
                content: AssistantContent::Parts(parts),
                ..
            }) => parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolResult(result) => Some(result),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for result in results {
            if let ToolResultOutput::Content { value, .. } = &result.output {
                for part in value {
                    if let ToolResultContentPart::File {
                        data: FileData::Url { url },
                        media_type,
                        ..
                    } = part
                    {
                        targets.push((url, media_type.clone()));
                    }
                }
            }
        }
        let Message::User(user) = message else {
            continue;
        };
        let UserContent::Parts(parts) = &user.content else {
            continue;
        };
        for part in parts {
            let (source, media_type) = match part {
                UserPart::Image(image) => (
                    &image.image,
                    image
                        .media_type
                        .clone()
                        .unwrap_or_else(|| MediaType::new("image/*")),
                ),
                UserPart::File(file) => (&file.data, file.media_type.clone()),
                _ => continue,
            };
            let FileSource::Url { url } = source else {
                continue;
            };
            targets.push((url, media_type));
        }
    }
    for (url, media_type) in targets {
        if url.scheme() == "data"
            || downloaded.contains_key(url.as_str())
            || !seen.insert(url.to_string())
        {
            continue;
        }
        requests.push(DownloadRequest {
            is_url_supported_by_model: ctx.supported_urls.supports(&media_type, url),
            url: url.clone(),
        });
    }
    if requests.is_empty()
        || (ctx.download.is_none()
            && requests
                .iter()
                .all(|request| request.is_url_supported_by_model))
    {
        return Ok(downloaded);
    }
    #[allow(
        clippy::needless_collect,
        reason = "the requests are moved into the download call before the urls are used"
    )]
    let urls: Vec<Url> = requests.iter().map(|request| request.url.clone()).collect();
    let results = match ctx.download {
        Some(download) => {
            download
                .download(requests, ctx.cancellation.clone())
                .await?
        }
        None => {
            let downloader = DefaultDownloader::try_default()?;
            downloader
                .download(requests, ctx.cancellation.clone())
                .await?
        }
    };
    downloaded.extend(
        urls.into_iter()
            .zip(results)
            .filter_map(|(url, file)| file.map(|file| (url.to_string(), file))),
    );
    if let Some(cache) = ctx.cache {
        cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(downloaded.clone());
    }
    Ok(downloaded)
}

async fn convert_user(
    user: &UserMessage,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<PromptMessage, Error> {
    let content = match &user.content {
        UserContent::Text(text) => vec![UserPromptPart::Text(TextPart::new(text.clone()))],
        UserContent::Parts(parts) => {
            let mut converted = Vec::with_capacity(parts.len());
            for part in parts {
                if matches!(part, UserPart::Text(text) if text.text.is_empty()) {
                    continue;
                }
                converted.push(convert_user_part(part, downloaded).await?);
            }
            converted
        }
        #[allow(unreachable_patterns, reason = "the content enum is non-exhaustive")]
        _ => Vec::new(),
    };
    Ok(PromptMessage::User {
        content,
        provider_options: user.provider_options.clone(),
    })
}

async fn convert_user_part(
    part: &UserPart,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<UserPromptPart, Error> {
    match part {
        UserPart::Text(text) => Ok(UserPromptPart::Text(text.clone())),
        UserPart::Image(image) => {
            let Resolved { data, media_type } = resolve_source(&image.image, downloaded).await?;
            let mut media_type =
                resolved_media_type(&image.image, image.media_type.as_ref(), media_type);
            if let Some(bytes) = data.as_bytes()
                && let Some(detected) = detect_media_type_for(bytes, "image")
            {
                media_type = Some(detected);
            }
            Ok(UserPromptPart::File(FilePart {
                data,
                media_type: media_type.unwrap_or_else(|| MediaType::new("image/*")),
                filename: None,
                provider_options: image.provider_options.clone(),
            }))
        }
        UserPart::File(file) => {
            let Resolved { data, media_type } = resolve_source(&file.data, downloaded).await?;
            let mut media_type =
                resolved_media_type(&file.data, Some(&file.media_type), media_type)
                    .unwrap_or_else(|| file.media_type.clone());
            if let Some(bytes) = data.as_bytes()
                && let Some(detected) = detect_media_type_for(bytes, "image")
            {
                media_type = detected;
            }
            Ok(UserPromptPart::File(FilePart {
                data,
                media_type,
                filename: file.filename.clone(),
                provider_options: file.provider_options.clone(),
            }))
        }
        #[allow(unreachable_patterns, reason = "the part enum is non-exhaustive")]
        _ => Err(Error::invalid_prompt("unsupported user message part")),
    }
}

fn resolved_media_type(
    source: &FileSource,
    declared: Option<&MediaType>,
    resolved: Option<MediaType>,
) -> Option<MediaType> {
    if matches!(source, FileSource::Url { url } if url.scheme() != "data")
        && declared.is_some_and(MediaType::is_full)
    {
        declared.cloned()
    } else {
        resolved.or_else(|| declared.cloned())
    }
}

struct Resolved {
    data: FileData,
    media_type: Option<MediaType>,
}

async fn resolve_source(
    source: &FileSource,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<Resolved, Error> {
    match source {
        FileSource::Bytes { data } => Ok(Resolved {
            data: FileData::bytes(data.clone()),
            media_type: None,
        }),
        FileSource::Base64 { .. } => {
            let bytes = source
                .decoded_bytes()
                .map_err(|error| {
                    Error::invalid_data_content(error.message.clone(), Some(Box::new(error)))
                })?
                .unwrap_or_default();
            Ok(Resolved {
                data: FileData::bytes(bytes),
                media_type: None,
            })
        }
        FileSource::Url { url } => {
            if let Some(parsed) = source.data_url() {
                let data_url = parsed.map_err(|error| {
                    Error::invalid_data_content(error.message.clone(), Some(Box::new(error)))
                })?;
                return Ok(Resolved {
                    data: FileData::bytes(data_url.data),
                    media_type: Some(data_url.media_type),
                });
            }
            if let Some(file) = downloaded.get(url.as_str()) {
                return Ok(Resolved {
                    data: FileData::bytes(file.data.clone()),
                    media_type: file.media_type.clone(),
                });
            }
            Ok(Resolved {
                data: FileData::url(url.clone()),
                media_type: None,
            })
        }
        FileSource::Reference { reference } => Ok(Resolved {
            data: FileData::Reference {
                reference: reference.clone(),
            },
            media_type: None,
        }),
        FileSource::Text { text } => Ok(Resolved {
            data: FileData::text(text.clone()),
            media_type: None,
        }),
        FileSource::Path { path } => {
            let bytes = tokio::fs::read(path).await.map_err(|error| {
                Error::invalid_data_content(
                    format!("failed to read file `{}`", path.display()),
                    Some(Box::new(error)),
                )
            })?;
            Ok(Resolved {
                data: FileData::bytes(Bytes::from(bytes)),
                media_type: None,
            })
        }
        #[allow(unreachable_patterns, reason = "the source enum is non-exhaustive")]
        _ => Err(Error::invalid_prompt("unsupported file source")),
    }
}

fn convert_assistant(
    assistant: &AssistantMessage,
    original: &Message,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<Option<PromptMessage>, Error> {
    let content = match &assistant.content {
        AssistantContent::Text(text) => {
            vec![AssistantPromptPart::Text(TextPart::new(text.clone()))]
        }
        AssistantContent::Parts(parts) => {
            let mut converted = Vec::with_capacity(parts.len());
            for part in parts {
                if let Some(part) = convert_assistant_part(part, original, downloaded)? {
                    converted.push(part);
                }
            }
            converted
        }
        #[allow(unreachable_patterns, reason = "the content enum is non-exhaustive")]
        _ => Vec::new(),
    };
    Ok(Some(PromptMessage::Assistant {
        content,
        provider_options: assistant.provider_options.clone(),
    }))
}

fn convert_assistant_part(
    part: &AssistantPart,
    original: &Message,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<Option<AssistantPromptPart>, Error> {
    let converted = match part {
        AssistantPart::Text(text) if text.text.is_empty() && text.provider_options.is_none() => {
            None
        }
        AssistantPart::Text(text) => Some(AssistantPromptPart::Text(text.clone())),
        AssistantPart::Custom(custom) => Some(AssistantPromptPart::Custom(custom.clone())),
        AssistantPart::Reasoning(reasoning) => {
            Some(AssistantPromptPart::Reasoning(reasoning.clone()))
        }
        AssistantPart::File(file) => {
            let resolved = file_data(file.data.clone(), original)?;
            Some(AssistantPromptPart::File(FilePart {
                data: resolved.data,
                media_type: resolved
                    .media_type
                    .unwrap_or_else(|| file.media_type.clone()),
                filename: file.filename.clone(),
                provider_options: file.provider_options.clone(),
            }))
        }
        AssistantPart::ReasoningFile(file) => {
            let resolved = file_data(file.data.clone(), original)?;
            if !matches!(resolved.data, FileData::Bytes { .. } | FileData::Url { .. }) {
                return Err(Error::MessageConversion {
                    message: "reasoning files require inline data or a URL".into(),
                    original_message: Box::new(original.clone()),
                });
            }
            Some(AssistantPromptPart::ReasoningFile(ReasoningFilePart {
                data: resolved.data,
                media_type: resolved
                    .media_type
                    .unwrap_or_else(|| file.media_type.clone()),
                provider_options: file.provider_options.clone(),
            }))
        }
        AssistantPart::ToolCall(call) => Some(AssistantPromptPart::ToolCall(call.clone())),
        AssistantPart::ToolResult(result) => Some(AssistantPromptPart::ToolResult(convert_result(
            result, downloaded,
        )?)),
        AssistantPart::ToolApprovalRequest(_) => None,
        #[allow(unreachable_patterns, reason = "the part enum is non-exhaustive")]
        _ => {
            return Err(Error::MessageConversion {
                message: "unsupported assistant message part".to_owned(),
                original_message: Box::new(original.clone()),
            });
        }
    };
    Ok(converted)
}

fn file_data(source: FileSource, original: &Message) -> Result<Resolved, Error> {
    if let Some(parsed) = source.data_url() {
        let parsed = parsed.map_err(|error| {
            Error::invalid_data_content(error.message.clone(), Some(Box::new(error)))
        })?;
        return Ok(Resolved {
            data: FileData::bytes(parsed.data),
            media_type: Some(parsed.media_type),
        });
    }
    let data = FileData::try_from(source).map_err(|error| Error::MessageConversion {
        message: error.to_string(),
        original_message: Box::new(original.clone()),
    })?;
    Ok(Resolved {
        data,
        media_type: None,
    })
}

fn convert_tool(
    tool: &ToolMessage,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<PromptMessage, Error> {
    let content: Vec<ToolPromptPart> = tool
        .content
        .iter()
        .filter_map(|part| match part {
            ToolPart::ToolResult(result) => {
                Some(convert_result(result, downloaded).map(ToolPromptPart::ToolResult))
            }
            ToolPart::ToolApprovalResponse(response) if response.provider_executed => Some(Ok(
                ToolPromptPart::ToolApprovalResponse(ToolApprovalResponsePart {
                    approval_id: response.approval_id.clone(),
                    approved: response.approved,
                    reason: response.reason.clone(),
                    provider_options: None,
                }),
            )),
            _ => None,
        })
        .collect::<Result<_, _>>()?;
    Ok(PromptMessage::Tool {
        content,
        provider_options: tool.provider_options.clone(),
    })
}

fn convert_result(
    result: &ToolResultPart,
    downloaded: &HashMap<String, DownloadedFile>,
) -> Result<ToolResultPart, Error> {
    let mut result = result.clone();
    if let ToolResultOutput::Content { value, .. } = &mut result.output {
        for part in value {
            if let ToolResultContentPart::File {
                data, media_type, ..
            } = part
            {
                if let FileData::Url { url } = data {
                    if url.scheme() == "data" {
                        let parsed =
                            ferrin_message::data_url::parse(url.as_str()).map_err(|error| {
                                Error::invalid_data_content(
                                    error.message.clone(),
                                    Some(Box::new(error)),
                                )
                            })?;
                        *data = FileData::bytes(parsed.data);
                        *media_type = parsed.media_type;
                    } else if let Some(file) = downloaded.get(url.as_str()) {
                        *data = FileData::bytes(file.data.clone());
                        if !media_type.is_full()
                            && let Some(detected) = &file.media_type
                        {
                            *media_type = detected.clone();
                        }
                    }
                }
                if let Some(bytes) = data.as_bytes()
                    && let Some(detected) = detect_media_type_for(bytes, "image")
                {
                    *media_type = detected;
                }
            }
        }
    }
    Ok(result)
}
