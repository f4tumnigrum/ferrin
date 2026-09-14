//! Usage and finish reason mapping.

use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::Usage;

use crate::api_types::AnthropicUsage;

/// Maps a stop reason to the unified finish reason.
#[must_use]
pub fn map_stop_reason(stop_reason: Option<&str>, json_response_from_tool: bool) -> FinishReason {
    let unified = match stop_reason {
        Some("pause_turn" | "end_turn" | "stop_sequence") => FinishReasonKind::Stop,
        Some("refusal") => FinishReasonKind::ContentFilter,
        Some("tool_use") if json_response_from_tool => FinishReasonKind::Stop,
        Some("tool_use") => FinishReasonKind::ToolCalls,
        Some("max_tokens" | "model_context_window_exceeded") => FinishReasonKind::Length,
        _ => FinishReasonKind::Other,
    };
    FinishReason {
        unified,
        raw: stop_reason.map(str::to_owned),
    }
}

/// Converts Anthropic usage to the specification usage.
///
/// Input total is `input + cache_creation + cache_read`. When per-iteration
/// usage is present and the request was not served by a fallback model, the
/// `compaction` and `message` iterations are summed instead of the top-level
/// counters. `raw` is kept as the provider-specific object.
#[must_use]
pub fn convert_usage(usage: &AnthropicUsage, raw: Option<JsonObject>) -> Usage {
    let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
    let reasoning = usage
        .output_tokens_details
        .as_ref()
        .and_then(|details| details.thinking_tokens);
    let mut input = usage.input_tokens.unwrap_or(0);
    let mut output = usage.output_tokens.unwrap_or(0);
    if let Some(iterations) = &usage.iterations
        && !iterations.is_empty()
        && !iterations
            .iter()
            .any(|iteration| iteration.kind == "fallback_message")
    {
        let executor: Vec<_> = iterations
            .iter()
            .filter(|iteration| matches!(iteration.kind.as_str(), "compaction" | "message"))
            .collect();
        if !executor.is_empty() {
            input = executor
                .iter()
                .map(|iteration| iteration.input_tokens.unwrap_or(0))
                .sum();
            output = executor
                .iter()
                .map(|iteration| iteration.output_tokens.unwrap_or(0))
                .sum();
        }
    }
    let mut result = Usage::totals(input + cache_write + cache_read, output);
    result.input.no_cache = Some(input);
    result.input.cache_read = Some(cache_read);
    result.input.cache_write = Some(cache_write);
    result.output.text = reasoning.map(|reasoning| output.saturating_sub(reasoning));
    result.output.reasoning = reasoning;
    result.raw = raw;
    result
}
