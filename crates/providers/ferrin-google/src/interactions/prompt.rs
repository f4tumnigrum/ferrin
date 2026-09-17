//! Interactions history conversion, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use ferrin_spec::FileData;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use serde_json::json;

use crate::config::GoogleConfig;

use super::request::Options;

fn option<'a>(
    config: &GoogleConfig,
    options: Option<&'a ProviderOptions>,
    key: &str,
) -> Option<&'a JsonValue> {
    options.and_then(|options| {
        options
            .get(config.options_key())
            .and_then(|v| v.get(key))
            .or_else(|| options.get("google").and_then(|v| v.get(key)))
    })
}

fn assistant_options(part: &AssistantPromptPart) -> Option<&ProviderOptions> {
    match part {
        AssistantPromptPart::Text(p) => p.provider_options.as_ref(),
        AssistantPromptPart::File(p) => p.provider_options.as_ref(),
        AssistantPromptPart::Reasoning(p) => p.provider_options.as_ref(),
        AssistantPromptPart::ToolCall(p) => p.provider_options.as_ref(),
        AssistantPromptPart::ToolResult(p) => p.provider_options.as_ref(),
        AssistantPromptPart::Custom(p) => p.provider_options.as_ref(),
        AssistantPromptPart::ReasoningFile(p) => p.provider_options.as_ref(),
        _ => None,
    }
}

pub(super) fn file(
    config: &GoogleConfig,
    data: &FileData,
    media: &str,
) -> Result<JsonValue, ProviderError> {
    if let FileData::Text { text } = data {
        return Ok(json!({"type":"text", "text":text}));
    }
    let kind = match media.split('/').next() {
        Some("image") => "image",
        Some("audio") => "audio",
        Some("video") => "video",
        Some("application" | "text") => "document",
        _ => {
            return Err(super::invalid(
                "media_type",
                "unsupported interactions file media type",
            ));
        }
    };
    let mut value = json!({"type":kind, "mime_type":media});
    match data {
        FileData::Bytes { .. } => value["data"] = json!(data.to_base64()),
        FileData::Url { url } => value["uri"] = json!(url),
        FileData::Reference { reference } => {
            let uri = reference
                .get(config.options_key())
                .or_else(|| reference.get("google"))
                .ok_or_else(|| super::invalid("reference", "missing Google file reference"))?;
            value["uri"] = json!(uri);
        }
        _ => {
            return Err(super::invalid(
                "file",
                "unsupported interactions file payload",
            ));
        }
    }
    Ok(value)
}

fn tool_result(
    config: &GoogleConfig,
    part: &ToolResultPart,
    warnings: &mut Vec<Warning>,
) -> Result<JsonValue, ProviderError> {
    let mut block =
        json!({"type":"function_result", "call_id":part.tool_call_id, "name":part.tool_name});
    block["result"] = match &part.output {
        ToolResultOutput::Text { value, .. } | ToolResultOutput::ErrorText { value, .. } => {
            json!(value)
        }
        ToolResultOutput::Json { value, .. } | ToolResultOutput::ErrorJson { value, .. } => {
            json!(value.to_string())
        }
        ToolResultOutput::ExecutionDenied { reason, .. } => {
            json!(reason.as_deref().unwrap_or("tool execution denied"))
        }
        ToolResultOutput::Content { value } => {
            let mut blocks = Vec::new();
            for part in value {
                match part {
                    ToolResultContentPart::Text { text, .. } => {
                        blocks.push(json!({"type":"text", "text":text}))
                    }
                    ToolResultContentPart::File {
                        data, media_type, ..
                    } if media_type.as_str().starts_with("image/") => {
                        blocks.push(file(config, data, media_type.as_str())?)
                    }
                    _ => warnings.push(Warning::unsupported("interactions tool result content")),
                }
            }
            json!(blocks)
        }
        _ => {
            return Err(super::invalid(
                "tool_result",
                "unsupported interactions tool output",
            ));
        }
    };
    if part.output.is_error() || matches!(part.output, ToolResultOutput::ExecutionDenied { .. }) {
        block["is_error"] = json!(true);
    }
    if let Some(signature) = option(config, part.provider_options.as_ref(), "signature") {
        block["signature"] = signature.clone();
    }
    Ok(block)
}

fn flush(steps: &mut Vec<JsonValue>, pending: &mut Vec<JsonValue>) {
    if !pending.is_empty() {
        steps.push(json!({"type":"model_output", "content":std::mem::take(pending)}));
    }
}

pub(super) fn convert(
    config: &GoogleConfig,
    call: &CallOptions,
    options: &Options,
    warnings: &mut Vec<Warning>,
) -> Result<(Vec<JsonValue>, Option<String>), ProviderError> {
    let mut steps = Vec::new();
    let mut system = Vec::new();
    if options.previous_interaction_id.is_some() && options.store == Some(false) {
        warnings.push(Warning::unsupported_with_details(
            "previousInteractionId with store=false",
            "full history is retained",
        ));
    }
    for message in &call.prompt {
        match message {
            PromptMessage::System { content, .. } => system.push(content.clone()),
            PromptMessage::User { content, .. } => {
                let mut blocks = Vec::new();
                for part in content {
                    match part {
                        UserPromptPart::Text(part) => {
                            blocks.push(json!({"type":"text", "text":part.text}))
                        }
                        UserPromptPart::File(part) => {
                            let mut block = file(config, &part.data, part.media_type.as_str())?;
                            if matches!(block["type"].as_str(), Some("image" | "video")) {
                                if let Some(resolution) = &options.media_resolution {
                                    block["resolution"] = json!(resolution);
                                }
                                if let Some(processing) =
                                    option(config, part.provider_options.as_ref(), "processing")
                                {
                                    block["processing"] = super::request::snake_fields(processing);
                                }
                            }
                            blocks.push(block);
                        }
                        _ => warnings.push(Warning::unsupported("interactions user part")),
                    }
                }
                if !blocks.is_empty() {
                    steps.push(json!({"type":"user_input", "content":blocks}));
                }
            }
            PromptMessage::Assistant {
                content,
                provider_options,
            } => {
                let linked = options
                    .previous_interaction_id
                    .as_deref()
                    .filter(|_| options.store != Some(false));
                if linked.is_some_and(|id| {
                    option(config, provider_options.as_ref(), "interactionId")
                        .and_then(JsonValue::as_str)
                        == Some(id)
                        || content.iter().any(|part| {
                            option(config, assistant_options(part), "interactionId")
                                .and_then(JsonValue::as_str)
                                == Some(id)
                        })
                }) {
                    continue;
                }
                let mut pending = Vec::new();
                for part in content {
                    let mut step = match part {
                        AssistantPromptPart::Text(part) => {
                            pending.push(json!({"type":"text", "text":part.text}));
                            continue;
                        }
                        AssistantPromptPart::File(part) => {
                            pending.push(file(config, &part.data, part.media_type.as_str())?);
                            continue;
                        }
                        AssistantPromptPart::Reasoning(part) => {
                            json!({"type":"thought", "summary":[{"type":"text", "text":part.text}]})
                        }
                        AssistantPromptPart::ToolCall(part) => {
                            let kind = option(config, part.provider_options.as_ref(), "stepType")
                                .and_then(JsonValue::as_str)
                                .unwrap_or("function_call");
                            json!({"type":kind, "id":part.tool_call_id, "name":part.tool_name, "arguments":part.input})
                        }
                        AssistantPromptPart::ToolResult(part) => {
                            let kind = option(config, part.provider_options.as_ref(), "stepType")
                                .and_then(JsonValue::as_str);
                            let result = tool_result(config, part, warnings)?;
                            if let Some(kind) = kind {
                                json!({"type":kind, "call_id":part.tool_call_id, "result":result["result"], "is_error":part.output.is_error()})
                            } else {
                                json!({"type":"user_input", "content":[result]})
                            }
                        }
                        AssistantPromptPart::Custom(part)
                            if matches!(
                                part.kind.as_str(),
                                "google.processing_call" | "google.processing_result"
                            ) =>
                        {
                            let metadata =
                                assistant_options(&AssistantPromptPart::Custom(part.clone()))
                                    .cloned()
                                    .unwrap_or_default();
                            let mut value = json!({"type":part.kind.kind()});
                            for (key, wire) in
                                [("processingId", "id"), ("processingCallId", "call_id")]
                            {
                                if let Some(id) = option(config, Some(&metadata), key) {
                                    value[wire] = id.clone();
                                }
                            }
                            value
                        }
                        _ => {
                            warnings.push(Warning::unsupported("interactions assistant part"));
                            continue;
                        }
                    };
                    flush(&mut steps, &mut pending);
                    if let Some(signature) = option(config, assistant_options(part), "signature") {
                        step["signature"] = signature.clone();
                    }
                    steps.push(step);
                }
                flush(&mut steps, &mut pending);
            }
            PromptMessage::Tool { content, .. } => {
                let mut blocks = Vec::new();
                for part in content {
                    if let ToolPromptPart::ToolResult(part) = part {
                        blocks.push(tool_result(config, part, warnings)?);
                    } else {
                        warnings.push(Warning::unsupported("interactions tool approval response"));
                    }
                }
                if !blocks.is_empty() {
                    steps.push(json!({"type":"user_input", "content":blocks}));
                }
            }
            _ => warnings.push(Warning::unsupported("interactions prompt role")),
        }
    }
    Ok((steps, (!system.is_empty()).then(|| system.join("\n\n"))))
}
