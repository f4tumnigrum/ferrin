//! Model capabilities inferred from the model id.

/// Feature flags of a model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// A `gemini-*` model.
    pub is_gemini: bool,
    /// A `gemini-2.5*` model (Gemini API drops `frequencyPenalty` and
    /// `presencePenalty` for these).
    pub is_gemini_2_5: bool,
    /// A `gemma-*` model (no `systemInstruction`).
    pub is_gemma: bool,
    /// Gemini 2 tools (`googleSearch`, `urlContext`, `codeExecution`,
    /// `enterpriseWebSearch`) are accepted.
    pub supports_gemini2_tools: bool,
    /// The `fileSearch` tool is accepted.
    pub supports_file_search: bool,
    /// Gemini 3 wire features: thinking levels, thought signatures on tool
    /// calls, mixed function and provider tools, `VALIDATED` tool mode.
    pub uses_gemini3_features: bool,
}

fn segment_has_prefix(model_id: &str, prefix: &str) -> bool {
    model_id.split('/').any(|segment| {
        segment
            .to_ascii_lowercase()
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '-']))
    })
}

fn segment_equals(model_id: &str, names: &[&str]) -> bool {
    model_id
        .split('/')
        .any(|segment| names.contains(&segment.to_ascii_lowercase().as_str()))
}

fn is_known_pre_gemini2(model_id: &str) -> bool {
    segment_has_prefix(model_id, "gemini-1")
        || segment_equals(model_id, &["gemini-pro", "gemini-pro-vision"])
        || segment_has_prefix(model_id, "gemini-robotics-er-1.5")
}

/// Infers the capabilities of `model_id`.
#[must_use]
pub fn capabilities(model_id: &str) -> ModelCapabilities {
    let lower = model_id.to_ascii_lowercase();
    let is_gemini = model_id
        .split('/')
        .any(|segment| segment.to_ascii_lowercase().starts_with("gemini-"));
    let is_gemini_2 = segment_has_prefix(model_id, "gemini-2");
    let pre_gemini2 = is_known_pre_gemini2(model_id);
    let uses_gemini3_features = is_gemini && !(pre_gemini2 || is_gemini_2);
    let is_gemini_2_5 = segment_has_prefix(model_id, "gemini-2.5");
    ModelCapabilities {
        is_gemini,
        is_gemini_2_5,
        is_gemma: lower.starts_with("gemma-"),
        supports_gemini2_tools: (is_gemini && !pre_gemini2) || lower.contains("nano-banana"),
        supports_file_search: is_gemini_2_5 || uses_gemini3_features,
        uses_gemini3_features,
    }
}

/// Maximum output tokens used when scaling a reasoning budget for Gemini 2.5.
pub const GEMINI_2_5_MAX_OUTPUT_TOKENS: u32 = 65_536;

/// Maximum thinking budget of a Gemini 2.5 model.
#[must_use]
pub fn max_thinking_tokens_gemini_2_5(model_id: &str) -> u32 {
    let lower = model_id.to_ascii_lowercase();
    if lower.contains("2.5-pro") || lower.contains("gemini-3-pro-image") {
        32_768
    } else {
        24_576
    }
}

/// Lowest thinking level a Gemini 3 model accepts: `low` for
/// `gemini-flash-latest` and for `gemini-X.Y-flash` (not `-lite`) from 3.7
/// on, otherwise `minimal`.
#[must_use]
pub fn minimum_thinking_level_gemini3(model_id: &str) -> &'static str {
    let name = model_id
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name == "gemini-flash-latest" {
        return "low";
    }
    let Some(rest) = name.strip_prefix("gemini-") else {
        return "minimal";
    };
    let Some((version, tail)) = rest.split_once("-flash") else {
        return "minimal";
    };
    let Some((major, minor)) = version.split_once('.') else {
        return "minimal";
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return "minimal";
    };
    let flash_variant = match tail.strip_prefix('-') {
        None if tail.is_empty() => true,
        None => false,
        Some(suffix) => !(suffix == "lite" || suffix.starts_with("lite-")),
    };
    if !flash_variant {
        return "minimal";
    }
    if major > 3 || (major == 3 && minor >= 7) {
        "low"
    } else {
        "minimal"
    }
}
