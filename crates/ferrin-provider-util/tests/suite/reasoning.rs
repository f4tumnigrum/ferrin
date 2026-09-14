use ferrin_provider_util::reasoning::BudgetPercentages;
use ferrin_provider_util::reasoning::is_custom_reasoning;
use ferrin_provider_util::reasoning::map_reasoning_to_budget;
use ferrin_provider_util::reasoning::map_reasoning_to_effort;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::Warning;
use pretty_assertions::assert_eq;

#[test]
fn effort_mapping_warns_on_indirect_and_unsupported() {
    let table = [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::XHigh, "high"),
    ];
    let mut warnings = Vec::new();
    assert_eq!(
        map_reasoning_to_effort(ReasoningEffort::Low, &table, &mut warnings),
        Some("low")
    );
    assert!(warnings.is_empty());

    assert_eq!(
        map_reasoning_to_effort(ReasoningEffort::XHigh, &table, &mut warnings),
        Some("high")
    );
    assert_eq!(
        warnings,
        vec![Warning::compatibility(
            "reasoning",
            Some(
                "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"high\"."
                    .to_owned()
            )
        )]
    );

    warnings.clear();
    assert_eq!(
        map_reasoning_to_effort(ReasoningEffort::Minimal, &table, &mut warnings),
        None
    );
    assert_eq!(
        warnings,
        vec![Warning::unsupported_with_details(
            "reasoning",
            "reasoning \"minimal\" is not supported by this model."
        )]
    );
    assert!(is_custom_reasoning(ReasoningEffort::Low));
    assert!(!is_custom_reasoning(ReasoningEffort::ProviderDefault));
}

#[test]
fn budget_mapping_scales_and_clamps() {
    let percentages = BudgetPercentages::default();
    let mut warnings = Vec::new();
    let cases = [
        (ReasoningEffort::Minimal, 1024),
        (ReasoningEffort::Low, 1600),
        (ReasoningEffort::Medium, 4800),
        (ReasoningEffort::High, 9600),
        (ReasoningEffort::XHigh, 12000),
    ];
    for (level, expected) in cases {
        assert_eq!(
            map_reasoning_to_budget(level, 16000, 12000, 1024, &percentages, &mut warnings),
            Some(expected),
            "{level:?}"
        );
    }
    assert!(warnings.is_empty());
    assert_eq!(
        map_reasoning_to_budget(
            ReasoningEffort::None,
            16000,
            12000,
            1024,
            &percentages,
            &mut warnings
        ),
        None
    );
    assert_eq!(warnings.len(), 1);
}
