//! Per-model defaults and feature support inferred from the model id.

/// Capabilities of a Claude model used for defaults and feature selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// Largest `max_tokens` the model accepts.
    pub max_output_tokens: u32,
    /// Whether `output_config.format` (native structured output) is
    /// available.
    pub supports_structured_output: bool,
    /// Whether `thinking: {type: adaptive}` is available.
    pub supports_adaptive_thinking: bool,
    /// Whether the model rejects `temperature`, `top_k` and `top_p`.
    pub rejects_sampling_parameters: bool,
    /// Whether `effort: xhigh` is accepted.
    pub supports_xhigh_effort: bool,
    /// Whether the model rejects `thinking: disabled` combined with an effort
    /// above `high`.
    pub rejects_thinking_disabled_above_high_effort: bool,
    /// Whether the id matches a known model family.
    pub is_known_model: bool,
}

const fn capabilities(
    max_output_tokens: u32,
    supports_structured_output: bool,
    supports_adaptive_thinking: bool,
    rejects_sampling_parameters: bool,
    supports_xhigh_effort: bool,
    rejects_thinking_disabled_above_high_effort: bool,
    is_known_model: bool,
) -> ModelCapabilities {
    ModelCapabilities {
        max_output_tokens,
        supports_structured_output,
        supports_adaptive_thinking,
        rejects_sampling_parameters,
        supports_xhigh_effort,
        rejects_thinking_disabled_above_high_effort,
        is_known_model,
    }
}

/// Whether `model_id` names the family `prefix` followed by a `-` or `@`
/// separator (so `claude-sonnet-4` does not match `claude-sonnet-4-5`).
fn family(model_id: &str, prefix: &str) -> bool {
    model_id.find(prefix).is_some_and(|start| {
        matches!(
            model_id.as_bytes().get(start + prefix.len()),
            Some(b'-' | b'@')
        )
    })
}

/// Whether `model_id` names a legacy Claude generation (`claude-instant`,
/// `claude-2`, `claude-v2`, `claude-3`).
fn legacy(model_id: &str) -> bool {
    let Some(rest) = model_id.strip_prefix("claude-") else {
        return false;
    };
    if rest == "instant" || rest.starts_with("instant-") {
        return true;
    }
    let rest = rest.strip_prefix('v').unwrap_or(rest);
    let ends_family =
        |text: &str, terminators: &[char]| text.is_empty() || text.starts_with(terminators);
    if let Some(after) = rest.strip_prefix('2') {
        return ends_family(after, &['-', '.', ':']);
    }
    if let Some(after) = rest.strip_prefix('3') {
        return ends_family(after, &['-', '.']);
    }
    false
}

/// Returns the capabilities of a model.
///
/// Unknown Claude ids are assumed newer than the known list and get the
/// most capable defaults with `is_known_model: false`; non-Claude ids keep
/// conservative defaults.
#[must_use]
pub fn model_capabilities(model_id: &str) -> ModelCapabilities {
    if model_id.contains("claude-opus-5") {
        capabilities(128_000, true, true, true, true, true, true)
    } else if model_id.contains("claude-opus-4-8")
        || model_id.contains("claude-opus-4-7")
        || model_id.contains("claude-fable-5")
        || model_id.contains("claude-sonnet-5")
    {
        capabilities(128_000, true, true, true, true, false, true)
    } else if model_id.contains("claude-sonnet-4-6") || model_id.contains("claude-opus-4-6") {
        capabilities(128_000, true, true, false, false, false, true)
    } else if model_id.contains("claude-sonnet-4-5")
        || model_id.contains("claude-opus-4-5")
        || model_id.contains("claude-haiku-4-5")
    {
        capabilities(64_000, true, false, false, false, false, true)
    } else if model_id.contains("claude-opus-4-1") {
        capabilities(32_000, true, false, false, false, false, true)
    } else if family(model_id, "claude-sonnet-4") {
        capabilities(64_000, false, false, false, false, false, true)
    } else if family(model_id, "claude-opus-4") {
        capabilities(32_000, false, false, false, false, false, true)
    } else if model_id.contains("claude-3-haiku") {
        capabilities(4_096, false, false, false, false, false, true)
    } else if legacy(model_id) {
        capabilities(4_096, false, false, false, false, false, false)
    } else if model_id.contains("claude-") {
        capabilities(128_000, true, true, true, true, true, false)
    } else {
        capabilities(4_096, false, false, false, false, false, false)
    }
}
