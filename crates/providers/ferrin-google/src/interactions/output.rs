//! Interactions output conversion, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use std::collections::BTreeMap;

use ferrin_spec::Content;
use ferrin_spec::FileData;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CustomKind;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::InputTokens;
use ferrin_spec::language_model::OutputTokens;
use ferrin_spec::language_model::ProviderToolResult;
use serde_json::json;
use url::Url;

use crate::config::GoogleConfig;

pub(super) fn metadata(config: &GoogleConfig, value: JsonValue) -> ProviderMetadata {
    let object = value.as_object().cloned().unwrap_or_default();
    let mut metadata = ProviderMetadata::new();
    metadata.insert("google".to_owned(), object.clone());
    metadata.insert(config.name.clone(), object);
    metadata
}

pub(super) fn part_metadata(
    config: &GoogleConfig,
    step: &JsonValue,
    interaction_id: Option<&str>,
) -> ProviderMetadata {
    let mut value = json!({});
    if let Some(id) = interaction_id.filter(|id| !id.is_empty()) {
        value["interactionId"] = json!(id);
    }
    if let Some(signature) = step.get("signature") {
        value["signature"] = signature.clone();
    }
    if let Some(kind) = step["type"].as_str() {
        value["stepType"] = json!(kind);
        if kind == "processing_call" {
            value["processingId"] = step["id"].clone();
        }
        if kind == "processing_result" {
            value["processingCallId"] = step["call_id"].clone();
        }
    }
    metadata(config, value)
}

pub(super) fn usage(value: &JsonValue) -> Usage {
    let input = value["total_input_tokens"].as_u64();
    let cache = value["total_cached_tokens"].as_u64();
    let output = value["total_output_tokens"].as_u64();
    let thought = value["total_thought_tokens"].as_u64();
    Usage {
        input: InputTokens {
            total: input,
            no_cache: input.map(|n| n.saturating_sub(cache.unwrap_or(0))),
            cache_read: cache,
            cache_write: None,
        },
        output: OutputTokens {
            total: if output.is_some() || thought.is_some() {
                Some(output.unwrap_or(0).saturating_add(thought.unwrap_or(0)))
            } else {
                None
            },
            text: output,
            reasoning: thought,
        },
        raw: value.as_object().cloned(),
    }
}

pub(super) fn finish(status: &str, function: bool) -> FinishReason {
    FinishReason::with_raw(
        match status {
            "requires_action" => FinishReasonKind::ToolCalls,
            "completed" if function => FinishReasonKind::ToolCalls,
            "completed" => FinishReasonKind::Stop,
            "incomplete" => FinishReasonKind::Length,
            "failed" => FinishReasonKind::Error,
            _ => FinishReasonKind::Other,
        },
        status,
    )
}

pub(super) fn block(
    config: &GoogleConfig,
    block: &JsonValue,
    metadata: &ProviderMetadata,
) -> Result<Vec<Content>, ProviderError> {
    let mut content = Vec::new();
    match block["type"].as_str() {
        Some("text") => {
            content.push(Content::Text {
                text: block["text"].as_str().unwrap_or_default().to_owned(),
                provider_metadata: Some(metadata.clone()),
            });
            content.extend(super::sources::extract(config, block));
        }
        Some(kind @ ("image" | "audio" | "video" | "document")) => {
            let data = if let Some(data) = block["data"].as_str().filter(|data| !data.is_empty()) {
                FileData::from_base64(data)
                    .map_err(|_| super::bad_response("invalid interactions file base64"))?
            } else if let Some(uri) = block["uri"].as_str() {
                FileData::url(
                    Url::parse(uri)
                        .map_err(|_| super::bad_response("invalid interactions file URL"))?,
                )
            } else {
                return Ok(content);
            };
            let default_media = match kind {
                "image" => "image/png",
                "audio" => "audio/pcm",
                "video" => "video/mp4",
                _ => "application/octet-stream",
            };
            content.push(Content::File {
                data,
                media_type: block["mime_type"].as_str().unwrap_or(default_media).into(),
                filename: None,
                provider_metadata: Some(metadata.clone()),
            });
        }
        _ => {}
    }
    Ok(content)
}

pub(super) fn step(
    config: &GoogleConfig,
    step: &JsonValue,
    interaction_id: Option<&str>,
    aliases: &BTreeMap<String, String>,
) -> Result<Vec<Content>, ProviderError> {
    let mut content = Vec::new();
    let metadata = part_metadata(config, step, interaction_id);
    let kind = step["type"].as_str().unwrap_or_default();
    match kind {
        "model_output" => {
            for item in step["content"].as_array().into_iter().flatten() {
                content.extend(block(config, item, &metadata)?);
            }
        }
        "thought" => content.push(Content::Reasoning {
            text: step["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            provider_metadata: Some(metadata),
        }),
        "processing_call" | "processing_result" => content.push(Content::Custom {
            kind: CustomKind::new("google", kind)
                .map_err(|_| super::bad_response("invalid interactions custom kind"))?,
            provider_metadata: Some(metadata),
        }),
        "function_call"
        | "google_search_call"
        | "code_execution_call"
        | "url_context_call"
        | "file_search_call"
        | "google_maps_call"
        | "mcp_server_tool_call" => {
            let builtin = kind != "function_call";
            if !builtin {
                for field in ["id", "name"] {
                    if step[field].as_str().is_none_or(str::is_empty) {
                        return Err(super::bad_response(
                            "interactions function call requires id and name",
                        ));
                    }
                }
                if step
                    .get("arguments")
                    .is_some_and(|arguments| !arguments.is_null() && !arguments.is_object())
                {
                    return Err(super::bad_response(
                        "interactions function arguments must be an object",
                    ));
                }
            }
            let wire_name = if !builtin || kind == "mcp_server_tool_call" {
                step["name"].as_str().unwrap_or("mcp_server_tool")
            } else {
                kind.trim_end_matches("_call")
            };
            let name = aliases.get(wire_name).map_or(wire_name, String::as_str);
            let mut call = ToolCall::new(
                step["id"]
                    .as_str()
                    .map_or_else(|| config.generate_id(), str::to_owned),
                name,
                step.get("arguments")
                    .filter(|arguments| !arguments.is_null())
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string(),
            );
            call.provider_executed = builtin;
            call.dynamic = kind == "mcp_server_tool_call";
            call.provider_metadata = Some(metadata);
            content.push(Content::ToolCall(call));
        }
        "google_search_result"
        | "code_execution_result"
        | "url_context_result"
        | "file_search_result"
        | "google_maps_result"
        | "mcp_server_tool_result" => {
            let wire_name = if kind == "mcp_server_tool_result" {
                step["name"].as_str().unwrap_or("mcp_server_tool")
            } else {
                kind.trim_end_matches("_result")
            };
            content.push(Content::ToolResult(ProviderToolResult {
                tool_call_id: step["call_id"]
                    .as_str()
                    .map_or_else(|| config.generate_id(), str::to_owned)
                    .into(),
                tool_name: aliases
                    .get(wire_name)
                    .map_or(wire_name, String::as_str)
                    .into(),
                result: step["result"].clone(),
                is_error: step["is_error"].as_bool().unwrap_or(false),
                preliminary: false,
                dynamic: kind == "mcp_server_tool_result",
                provider_metadata: Some(metadata),
            }));
            content.extend(super::sources::extract(config, step));
        }
        _ => {}
    }
    Ok(content)
}

pub(super) fn response_metadata(config: &GoogleConfig, value: &JsonValue) -> ProviderMetadata {
    let mut metadata_value = json!({});
    if let Some(id) = value["id"].as_str().filter(|id| !id.is_empty()) {
        metadata_value["interactionId"] = json!(id);
    }
    if let Some(tier) = value.get("service_tier") {
        metadata_value["serviceTier"] = tier.clone();
    }
    let by_modality: JsonObject = value["usage"]["output_tokens_by_modality"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                entry["modality"].as_str()?.to_owned(),
                entry.get("tokens")?.clone(),
            ))
        })
        .collect();
    if !by_modality.is_empty() {
        metadata_value["outputTokensByModality"] = json!(by_modality);
    }
    metadata(config, metadata_value)
}

pub(super) fn convert(
    config: &GoogleConfig,
    value: &JsonValue,
    aliases: &BTreeMap<String, String>,
) -> Result<GenerateResult, ProviderError> {
    let status = value["status"]
        .as_str()
        .ok_or_else(|| super::bad_response("interactions response omitted status"))?;
    let mut content = Vec::new();
    for item in value["steps"].as_array().into_iter().flatten() {
        content.extend(step(config, item, value["id"].as_str(), aliases)?);
    }
    let function = content
        .iter()
        .any(|part| matches!(part, Content::ToolCall(call) if !call.provider_executed));
    let mut result = GenerateResult::new(content, finish(status, function));
    result.usage = usage(&value["usage"]);
    result.provider_metadata = Some(response_metadata(config, value));
    result.response.id = value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    result.response.model_id = value["model"].as_str().map(Into::into);
    result.response.timestamp = value["created"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|time| time.with_timezone(&chrono::Utc));
    result.response.body = Some(value.clone());
    Ok(result)
}
