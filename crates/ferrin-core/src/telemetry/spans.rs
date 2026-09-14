//! Built-in `tracing` spans and events following the GenAI semantic
//! conventions (`gen_ai.*`); Ferrin-specific fields use the `ferrin.*`
//! prefix.

use ferrin_spec::Warning;
use tracing::Span;

use super::ModelIdentity;

/// Target used for adapter warnings.
pub const WARNINGS_TARGET: &str = "ferrin::warnings";

/// Span for a `generate_text` or `stream_text` call.
pub(crate) fn call_span(
    operation: &'static str,
    function_id: Option<&str>,
    model: &ModelIdentity,
) -> Span {
    tracing::info_span!(
        "ferrin.call",
        "gen_ai.operation.name" = operation,
        "ferrin.function_id" = function_id.unwrap_or(""),
        "gen_ai.request.model" = %model.model_id,
        "gen_ai.provider.name" = %model.provider,
    )
}

/// Span for one step.
pub(crate) fn step_span(step_number: u32) -> Span {
    tracing::info_span!("ferrin.step", "ferrin.step_number" = step_number)
}

/// Span for one model call.
pub(crate) fn model_call_span(model: &ModelIdentity) -> Span {
    tracing::info_span!(
        "ferrin.model_call",
        "gen_ai.request.model" = %model.model_id,
        "gen_ai.provider.name" = %model.provider,
        "gen_ai.response.id" = tracing::field::Empty,
        "gen_ai.response.finish_reasons" = tracing::field::Empty,
        "gen_ai.usage.input_tokens" = tracing::field::Empty,
        "gen_ai.usage.output_tokens" = tracing::field::Empty,
        "ferrin.time_to_first_output_ms" = tracing::field::Empty,
    )
}

/// Span for one tool execution.
pub(crate) fn tool_span(tool_name: &str, tool_call_id: &str) -> Span {
    tracing::info_span!(
        "ferrin.tool",
        "gen_ai.tool.name" = tool_name,
        "gen_ai.tool.call.id" = tool_call_id,
        "ferrin.tool.duration_ms" = tracing::field::Empty,
    )
}

/// Span for a non-text modality (`embed`, `rerank`, `image`, ...).
pub(crate) fn modality_span(operation: &'static str, model: &ModelIdentity) -> Span {
    tracing::info_span!(
        "ferrin.modality",
        "gen_ai.operation.name" = operation,
        "gen_ai.request.model" = %model.model_id,
        "gen_ai.provider.name" = %model.provider,
    )
}

/// Logs adapter warnings, one event per warning.
pub(crate) fn log_warnings(warnings: &[Warning], model: &ModelIdentity) {
    for warning in warnings {
        let (kind, feature, details) = match warning {
            Warning::Unsupported { feature, details } => {
                ("unsupported", feature.as_str(), details.as_deref())
            }
            Warning::Compatibility { feature, details } => {
                ("compatibility", feature.as_str(), details.as_deref())
            }
            Warning::Deprecated { setting, message } => {
                ("deprecated", setting.as_str(), Some(message.as_str()))
            }
            Warning::Other { message } => ("other", "", Some(message.as_str())),
            #[allow(unreachable_patterns, reason = "the warning enum is non-exhaustive")]
            _ => ("other", "", None),
        };
        tracing::warn!(
            target: WARNINGS_TARGET,
            category = kind,
            feature = feature,
            details = details.unwrap_or(""),
            provider = %model.provider,
            model_id = %model.model_id,
            "provider warning"
        );
    }
}
