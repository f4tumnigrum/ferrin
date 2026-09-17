//! Mapping of Chat Completions responses to specification types.

use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::Usage;

use super::api_types::ChatUsage;

/// Maps a `finish_reason` string.
#[must_use]
pub fn map_chat_finish_reason(reason: &str) -> FinishReason {
    let unified = match reason {
        "stop" => FinishReasonKind::Stop,
        "length" => FinishReasonKind::Length,
        "content_filter" => FinishReasonKind::ContentFilter,
        "function_call" | "tool_calls" => FinishReasonKind::ToolCalls,
        _ => FinishReasonKind::Other,
    };
    FinishReason::with_raw(unified, reason)
}

/// Maps usage.
#[must_use]
pub fn map_chat_usage(usage: &ChatUsage, raw: Option<JsonObject>) -> Usage {
    let input = usage.prompt_tokens.unwrap_or(0);
    let output = usage.completion_tokens.unwrap_or(0);
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens);
    let cache_write = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cache_write_tokens);
    let reasoning = usage
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens);
    let mut result = Usage::totals(input, output);
    result.input.no_cache = Some(
        input
            .saturating_sub(cached.unwrap_or(0))
            .saturating_sub(cache_write.unwrap_or(0)),
    );
    result.input.cache_read = Some(cached.unwrap_or(0));
    result.input.cache_write = cache_write;
    result.output.text = Some(output.saturating_sub(reasoning.unwrap_or(0)));
    result.output.reasoning = Some(reasoning.unwrap_or(0));
    result.raw = raw;
    result
}

/// Prediction token counts for the provider metadata.
#[must_use]
pub fn prediction_metadata(usage: &ChatUsage) -> Vec<(String, u64)> {
    let Some(details) = &usage.completion_tokens_details else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(accepted) = details.accepted_prediction_tokens {
        out.push(("acceptedPredictionTokens".to_owned(), accepted));
    }
    if let Some(rejected) = details.rejected_prediction_tokens {
        out.push(("rejectedPredictionTokens".to_owned(), rejected));
    }
    out
}
