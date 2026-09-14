//! Conversion of application messages into the provider prompt.

use std::collections::HashMap;

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
use ferrin_spec::language_model::prompt::UserPromptPart;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::Instructions;
use super::download::DefaultDownloader;
use super::download::DownloadFn;
use super::download::DownloadRequest;
use super::download::DownloadedFile;
use crate::error::Error;

/// Inputs of a conversion.
pub(crate) struct ConvertContext<'a> {
    /// URL patterns the model fetches itself.
    pub(crate) supported_urls: &'a SupportedUrls,
    /// Download function; `None` uses the default downloader.
    pub(crate) download: Option<&'a dyn DownloadFn>,
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
        prompt.push(PromptMessage::System {
            content: system.content.clone(),
            provider_options: system.provider_options.clone(),
        });
    }
    for message in messages {
        let converted = match message {
            Message::System(system) => Some(PromptMessage::System {
                content: system.content.clone(),
                provider_options: system.provider_options.clone(),
            }),
            Message::User(user) => Some(convert_user(user, &downloaded).await?),
            Message::Assistant(assistant) => convert_assistant(assistant, message)?,
            Message::Tool(tool) => convert_tool(tool),
            #[allow(unreachable_patterns, reason = "the message enum is non-exhaustive")]
            _ => {
                return Err(Error::MessageConversion {
                    message: "unsupported message role".to_owned(),
                    original_message: Box::new(message.clone()),
                });
            }
        };
        prompt.extend(converted);
    }
    Ok(prompt)
}

async fn download_files(
    messages: &[Message],
    ctx: &ConvertContext<'_>,
) -> Result<HashMap<String, DownloadedFile>, Error> {
    let mut requests: Vec<DownloadRequest> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for message in messages {
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
            if url.scheme() == "data" || !seen.insert(url.to_string()) {
                continue;
            }
            requests.push(DownloadRequest {
                is_url_supported_by_model: ctx.supported_urls.supports(&media_type, url),
                url: url.clone(),
            });
        }
    }
    if requests.is_empty()
        || requests
            .iter()
            .all(|request| request.is_url_supported_by_model)
    {
        return Ok(HashMap::new());
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
    Ok(urls
        .into_iter()
        .zip(results)
        .filter_map(|(url, file)| file.map(|file| (url.to_string(), file)))
        .collect())
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
            let mut media_type = media_type.or_else(|| image.media_type.clone());
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
            Ok(UserPromptPart::File(FilePart {
                data,
                media_type: media_type.unwrap_or_else(|| file.media_type.clone()),
                filename: file.filename.clone(),
                provider_options: file.provider_options.clone(),
            }))
        }
        #[allow(unreachable_patterns, reason = "the part enum is non-exhaustive")]
        _ => Err(Error::invalid_prompt("unsupported user message part")),
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
) -> Result<Option<PromptMessage>, Error> {
    let content = match &assistant.content {
        AssistantContent::Text(text) => {
            vec![AssistantPromptPart::Text(TextPart::new(text.clone()))]
        }
        AssistantContent::Parts(parts) => {
            let mut converted = Vec::with_capacity(parts.len());
            for part in parts {
                if let Some(part) = convert_assistant_part(part, original)? {
                    converted.push(part);
                }
            }
            converted
        }
        #[allow(unreachable_patterns, reason = "the content enum is non-exhaustive")]
        _ => Vec::new(),
    };
    if content.is_empty() {
        return Ok(None);
    }
    Ok(Some(PromptMessage::Assistant {
        content,
        provider_options: assistant.provider_options.clone(),
    }))
}

fn convert_assistant_part(
    part: &AssistantPart,
    original: &Message,
) -> Result<Option<AssistantPromptPart>, Error> {
    let converted = match part {
        AssistantPart::Text(text) if text.text.is_empty() => None,
        AssistantPart::Text(text) => Some(AssistantPromptPart::Text(text.clone())),
        AssistantPart::Custom(custom) => Some(AssistantPromptPart::Custom(custom.clone())),
        AssistantPart::Reasoning(reasoning) => {
            Some(AssistantPromptPart::Reasoning(reasoning.clone()))
        }
        AssistantPart::File(file) => Some(AssistantPromptPart::File(FilePart {
            data: file_data(file.data.clone(), original)?,
            media_type: file.media_type.clone(),
            filename: file.filename.clone(),
            provider_options: file.provider_options.clone(),
        })),
        AssistantPart::ReasoningFile(file) => {
            Some(AssistantPromptPart::ReasoningFile(ReasoningFilePart {
                data: file_data(file.data.clone(), original)?,
                media_type: file.media_type.clone(),
                provider_options: file.provider_options.clone(),
            }))
        }
        AssistantPart::ToolCall(call) => Some(AssistantPromptPart::ToolCall(call.clone())),
        AssistantPart::ToolResult(result) => Some(AssistantPromptPart::ToolResult(result.clone())),
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

fn file_data(source: FileSource, original: &Message) -> Result<FileData, Error> {
    FileData::try_from(source).map_err(|error| Error::MessageConversion {
        message: error.to_string(),
        original_message: Box::new(original.clone()),
    })
}

fn convert_tool(tool: &ToolMessage) -> Option<PromptMessage> {
    let content: Vec<ToolPromptPart> = tool
        .content
        .iter()
        .filter_map(|part| match part {
            ToolPart::ToolResult(result) => Some(ToolPromptPart::ToolResult(result.clone())),
            ToolPart::ToolApprovalResponse(response) if response.provider_executed => Some(
                ToolPromptPart::ToolApprovalResponse(ToolApprovalResponsePart {
                    approval_id: response.approval_id.clone(),
                    approved: response.approved,
                    reason: response.reason.clone(),
                    provider_options: None,
                }),
            ),
            _ => None,
        })
        .collect();
    if content.is_empty() {
        return None;
    }
    Some(PromptMessage::Tool {
        content,
        provider_options: tool.provider_options.clone(),
    })
}
