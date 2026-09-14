//! Mapping of the unified reasoning level to provider settings.

use ferrin_spec::ReasoningEffort;
use ferrin_spec::Warning;

/// Returns `true` when the application asked for a specific level (anything
/// but the provider default).
#[must_use]
pub fn is_custom_reasoning(reasoning: ReasoningEffort) -> bool {
    reasoning != ReasoningEffort::ProviderDefault
}

/// Maps a reasoning level through a provider effort table.
///
/// Levels missing from the table produce an `unsupported` warning and
/// `None`; levels mapped to a different name produce a `compatibility`
/// warning.
pub fn map_reasoning_to_effort<T: Clone + AsRef<str>>(
    reasoning: ReasoningEffort,
    effort_map: &[(ReasoningEffort, T)],
    warnings: &mut Vec<Warning>,
) -> Option<T> {
    let mapped = effort_map
        .iter()
        .find(|(level, _)| *level == reasoning)
        .map(|(_, value)| value.clone());
    let Some(mapped) = mapped else {
        warnings.push(Warning::unsupported_with_details(
            "reasoning",
            format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            ),
        ));
        return None;
    };
    if mapped.as_ref() != reasoning.as_str() {
        warnings.push(Warning::compatibility(
            "reasoning",
            Some(format!(
                "reasoning \"{}\" is not directly supported by this model. mapped to effort \"{}\".",
                reasoning.as_str(),
                mapped.as_ref()
            )),
        ));
    }
    Some(mapped)
}

/// Share of `max_output_tokens` granted to reasoning per level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BudgetPercentages {
    /// `minimal`.
    pub minimal: Option<f64>,
    /// `low`.
    pub low: Option<f64>,
    /// `medium`.
    pub medium: Option<f64>,
    /// `high`.
    pub high: Option<f64>,
    /// `xhigh`.
    pub xhigh: Option<f64>,
}

impl Default for BudgetPercentages {
    fn default() -> Self {
        Self {
            minimal: Some(0.02),
            low: Some(0.1),
            medium: Some(0.3),
            high: Some(0.6),
            xhigh: Some(0.9),
        }
    }
}

impl BudgetPercentages {
    fn for_level(&self, reasoning: ReasoningEffort) -> Option<f64> {
        match reasoning {
            ReasoningEffort::Minimal => self.minimal,
            ReasoningEffort::Low => self.low,
            ReasoningEffort::Medium => self.medium,
            ReasoningEffort::High => self.high,
            ReasoningEffort::XHigh => self.xhigh,
            _ => None,
        }
    }
}

/// Default lower bound of a reasoning budget.
pub const DEFAULT_MIN_REASONING_BUDGET: u32 = 1024;

/// Maps a reasoning level to a token budget:
/// `clamp(round(max_output_tokens * pct), min_budget, max_budget)`.
///
/// Levels without a percentage produce an `unsupported` warning and `None`.
pub fn map_reasoning_to_budget(
    reasoning: ReasoningEffort,
    max_output_tokens: u32,
    max_budget: u32,
    min_budget: u32,
    percentages: &BudgetPercentages,
    warnings: &mut Vec<Warning>,
) -> Option<u32> {
    let Some(pct) = percentages.for_level(reasoning) else {
        warnings.push(Warning::unsupported_with_details(
            "reasoning",
            format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            ),
        ));
        return None;
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the product of a u32 and a share in [0, 1] fits in u32"
    )]
    let scaled = (f64::from(max_output_tokens) * pct).round() as u32;
    Some(scaled.max(min_budget).min(max_budget))
}
