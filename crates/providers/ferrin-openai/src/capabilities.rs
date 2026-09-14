//! Capability detection from model ids.

/// System message handling of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemMessageMode {
    /// Send as `system`.
    System,
    /// Send as `developer`.
    Developer,
    /// Drop with a warning.
    Remove,
}

/// Capabilities derived from a model id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// Reasoning model (o-series, GPT-5 and later except chat variants).
    pub is_reasoning_model: bool,
    /// Default system message mode.
    pub system_message_mode: SystemMessageMode,
    /// Accepts `service_tier: "flex"`.
    pub supports_flex_processing: bool,
    /// Accepts `service_tier: "priority"` / `"fast"`.
    pub supports_priority_processing: bool,
    /// Accepts `configuration_update` input items (GPT-6 and later).
    pub supports_configuration_update: bool,
    /// Accepts asynchronous function tools (GPT-6 and later).
    pub supports_async_tool_calling: bool,
    /// Restricted reasoning effort values, when the model defines them.
    pub supported_reasoning_efforts: Option<&'static [&'static str]>,
    /// Accepts sampling parameters when reasoning is `none`.
    pub supports_non_reasoning_parameters: bool,
}

impl ModelCapabilities {
    /// Detects the capabilities of `model_id`.
    #[must_use]
    pub fn for_model(model_id: &str) -> Self {
        let o_series = o_series_version(model_id);
        let gpt = gpt_version(model_id);
        let is_chat = gpt.is_some_and(|g| g.minor.is_none() && g.variant.starts_with("chat"));
        let is_nano = gpt.is_some_and(|g| g.variant.starts_with("nano"));
        let is_gpt6_or_later = gpt.is_some_and(|g| g.major >= 6);
        let supports_flex_processing =
            o_series.is_some_and(|v| v >= 3) || gpt.is_some_and(|g| g.major >= 5 && !is_chat);
        let supports_priority_processing = model_id.starts_with("gpt-4")
            || gpt.is_some_and(|g| g.major >= 5 && !is_nano && !is_chat)
            || o_series.is_some_and(|v| v >= 3);
        let is_reasoning_model =
            o_series.is_some() || gpt.is_some_and(|g| g.major >= 5 && !is_chat);
        let supports_non_reasoning_parameters = !is_gpt6_or_later
            && gpt.is_some_and(|g| g.major > 5 || (g.major == 5 && g.minor.unwrap_or(0) >= 1));
        Self {
            is_reasoning_model,
            system_message_mode: if is_reasoning_model {
                SystemMessageMode::Developer
            } else {
                SystemMessageMode::System
            },
            supports_flex_processing,
            supports_priority_processing,
            supports_configuration_update: is_gpt6_or_later,
            supports_async_tool_calling: is_gpt6_or_later,
            supported_reasoning_efforts: is_gpt6_or_later
                .then_some(&["low", "medium", "high", "xhigh", "max"][..]),
            supports_non_reasoning_parameters,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GptVersion<'a> {
    major: u32,
    minor: Option<u32>,
    variant: &'a str,
}

/// `o<N>` or `o<N>-...` → `N`.
fn o_series_version(model_id: &str) -> Option<u32> {
    let rest = model_id.strip_prefix('o')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let after = &rest[digits.len()..];
    (after.is_empty() || after.starts_with('-')).then(|| digits.parse().ok())?
}

/// `gpt-<major>[.<minor>][-<variant>]`.
fn gpt_version(model_id: &str) -> Option<GptVersion<'_>> {
    let rest = model_id.strip_prefix("gpt-")?;
    let major_digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if major_digits.is_empty() {
        return None;
    }
    let major = major_digits.parse().ok()?;
    let mut rest = &rest[major_digits.len()..];
    let mut minor = None;
    if let Some(after_dot) = rest.strip_prefix('.') {
        let minor_digits: String = after_dot.chars().take_while(char::is_ascii_digit).collect();
        if minor_digits.is_empty() {
            return None;
        }
        minor = Some(minor_digits.parse().ok()?);
        rest = &after_dot[minor_digits.len()..];
    }
    let variant = match rest {
        "" => "",
        _ => rest.strip_prefix('-')?,
    };
    Some(GptVersion {
        major,
        minor,
        variant,
    })
}
