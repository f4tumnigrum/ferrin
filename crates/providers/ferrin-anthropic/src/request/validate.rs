//! Validation of provider options and resolution of the unified reasoning
//! level.

use ferrin_provider_util::reasoning::BudgetPercentages;
use ferrin_provider_util::reasoning::DEFAULT_MIN_REASONING_BUDGET;
use ferrin_provider_util::reasoning::map_reasoning_to_budget;
use ferrin_provider_util::reasoning::map_reasoning_to_effort;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;

use crate::options::AnthropicLanguageModelOptions;
use crate::options::Fallbacks;
use crate::options::Thinking;

fn validate_enum(
    field: &str,
    value: Option<&str>,
    allowed: &[&str],
) -> Result<(), InvalidArgumentError> {
    match value {
        Some(value) if !allowed.contains(&value) => Err(InvalidArgumentError::new(
            "provider_options",
            format!(
                "invalid value \"{value}\" for {field}; expected one of {}",
                allowed.join(", ")
            ),
        )),
        _ => Ok(()),
    }
}

pub(super) fn validate_options(
    options: &AnthropicLanguageModelOptions,
) -> Result<(), InvalidArgumentError> {
    validate_enum(
        "structuredOutputMode",
        options.structured_output_mode.as_deref(),
        &["outputFormat", "jsonTool", "auto"],
    )?;
    validate_enum(
        "effort",
        options.effort.as_deref(),
        &["low", "medium", "high", "xhigh", "max"],
    )?;
    validate_enum("speed", options.speed.as_deref(), &["fast", "standard"])?;
    validate_enum(
        "serviceTier",
        options.service_tier.as_deref(),
        &["auto", "standard_only"],
    )?;
    validate_enum(
        "inferenceGeo",
        options.inference_geo.as_deref(),
        &["us", "global"],
    )?;
    if let Some(thinking) = &options.thinking {
        validate_enum(
            "thinking.type",
            thinking.kind.as_deref(),
            &["adaptive", "enabled", "disabled"],
        )?;
        validate_enum(
            "thinking.display",
            thinking.display.as_deref(),
            &["omitted", "summarized", "updates"],
        )?;
        if let Some(binding) = &thinking.block_binding {
            validate_enum(
                "thinking.blockBinding.prefixMismatchBehavior",
                Some(binding.prefix_mismatch_behavior.as_str()),
                &["error", "drop_block"],
            )?;
        }
    }
    if let Some(cache) = &options.cache_control {
        validate_enum(
            "cacheControl.type",
            Some(cache.kind.as_str()),
            &["ephemeral"],
        )?;
        validate_enum("cacheControl.ttl", cache.ttl.as_deref(), &["5m", "1h"])?;
    }
    if let Some(budget) = &options.task_budget {
        validate_enum("taskBudget.type", Some(budget.kind.as_str()), &["tokens"])?;
        if budget.total < 20_000 {
            return Err(InvalidArgumentError::new(
                "provider_options",
                "taskBudget.total must be at least 20000",
            ));
        }
    }
    if let Some(Fallbacks::Default(value)) = &options.fallbacks
        && value != "default"
    {
        return Err(InvalidArgumentError::new(
            "provider_options",
            format!("invalid value \"{value}\" for fallbacks; expected \"default\" or a list"),
        ));
    }
    Ok(())
}

/// Resolves the unified reasoning level into thinking/effort options.
pub(super) fn resolve_reasoning(
    reasoning: ReasoningEffort,
    supports_adaptive_thinking: bool,
    supports_xhigh_effort: bool,
    max_output_tokens_for_model: u32,
    warnings: &mut Vec<Warning>,
) -> (Option<Thinking>, Option<String>) {
    if reasoning == ReasoningEffort::None {
        return (
            Some(Thinking {
                kind: Some("disabled".to_owned()),
                ..Thinking::default()
            }),
            None,
        );
    }
    if supports_adaptive_thinking {
        let xhigh = if supports_xhigh_effort {
            "xhigh"
        } else {
            "max"
        };
        let effort = map_reasoning_to_effort(
            reasoning,
            &[
                (ReasoningEffort::Minimal, "low"),
                (ReasoningEffort::Low, "low"),
                (ReasoningEffort::Medium, "medium"),
                (ReasoningEffort::High, "high"),
                (ReasoningEffort::XHigh, xhigh),
            ],
            warnings,
        );
        return (
            Some(Thinking {
                kind: Some("adaptive".to_owned()),
                display: Some("summarized".to_owned()),
                ..Thinking::default()
            }),
            effort.map(str::to_owned),
        );
    }
    let budget = map_reasoning_to_budget(
        reasoning,
        max_output_tokens_for_model,
        max_output_tokens_for_model,
        DEFAULT_MIN_REASONING_BUDGET,
        &BudgetPercentages::default(),
        warnings,
    );
    match budget {
        Some(budget) => (
            Some(Thinking {
                kind: Some("enabled".to_owned()),
                budget_tokens: Some(budget),
                ..Thinking::default()
            }),
            None,
        ),
        None => (None, None),
    }
}
