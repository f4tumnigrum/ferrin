//! Mapping of chat responses to specification types.

use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Usage;
use ferrin_spec::language_model::Content;

use super::api_types::ChatContent;
use super::api_types::ChatUsage;

/// Maps a `finish_reason` string.
#[must_use]
pub fn map_finish_reason(reason: &str) -> FinishReason {
    let unified = match reason {
        "stop" => FinishReasonKind::Stop,
        "length" => FinishReasonKind::Length,
        "content_filter" => FinishReasonKind::ContentFilter,
        "function_call" | "tool_calls" => FinishReasonKind::ToolCalls,
        _ => FinishReasonKind::Other,
    };
    FinishReason::with_raw(unified, reason)
}

/// Maps usage: `total = prompt_tokens`, `no_cache = prompt_tokens -
/// cached_tokens`, `text = completion_tokens - reasoning_tokens`.
#[must_use]
pub fn convert_usage(usage: &ChatUsage, raw: Option<JsonObject>) -> Usage {
    let input = usage.prompt_tokens.unwrap_or(0);
    let output = usage.completion_tokens.unwrap_or(0);
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);
    let reasoning = usage
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens)
        .unwrap_or(0);
    let mut result = Usage::totals(input, output);
    result.input.no_cache = Some(input.saturating_sub(cached));
    result.input.cache_read = Some(cached);
    result.output.text = Some(output.saturating_sub(reasoning));
    result.output.reasoning = Some(reasoning);
    result.raw = raw;
    result
}

/// Prediction token counts for the provider metadata.
#[must_use]
pub fn prediction_metadata(usage: &ChatUsage) -> JsonObject {
    let mut out = JsonObject::new();
    let Some(details) = &usage.completion_tokens_details else {
        return out;
    };
    if let Some(accepted) = details.accepted_prediction_tokens {
        out.insert(
            "acceptedPredictionTokens".to_owned(),
            JsonValue::from(accepted),
        );
    }
    if let Some(rejected) = details.rejected_prediction_tokens {
        out.insert(
            "rejectedPredictionTokens".to_owned(),
            JsonValue::from(rejected),
        );
    }
    out
}

/// Converts message content: a string becomes one text part; typed parts
/// become text (`text`) and reasoning (`thinking`) parts. Empty text is
/// dropped.
#[must_use]
pub fn convert_content(content: Option<&ChatContent>) -> Vec<Content> {
    match content {
        None => Vec::new(),
        Some(ChatContent::Text(text)) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Content::text(text.clone())]
            }
        }
        Some(ChatContent::Parts(parts)) => parts.iter().filter_map(convert_part).collect(),
    }
}

fn convert_part(part: &JsonObject) -> Option<Content> {
    match part.get("type").and_then(JsonValue::as_str)? {
        "text" => {
            let text = part.get("text")?.as_str()?;
            (!text.is_empty()).then(|| Content::text(text))
        }
        "thinking" => {
            let reasoning: String = part
                .get("thinking")?
                .as_array()?
                .iter()
                .filter(|chunk| chunk.get("type").and_then(JsonValue::as_str) == Some("text"))
                .filter_map(|chunk| chunk.get("text").and_then(JsonValue::as_str))
                .collect();
            (!reasoning.is_empty()).then(|| Content::reasoning(reasoning))
        }
        _ => None,
    }
}
