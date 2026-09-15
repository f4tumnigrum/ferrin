use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use ferrin_spec::error::ProviderError;
use ferrin_testing::StreamContractChecker;
use ferrin_testing::simulate_stream;
use ferrin_testing::text_parts;
use pretty_assertions::assert_eq;

fn messages(result: Result<(), Vec<ferrin_testing::ContractViolation>>) -> Vec<String> {
    result
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|violation| violation.to_string())
        .collect()
}

#[test]
fn valid_text_stream_passes() {
    let parts = text_parts(["a"], Usage::totals(1, 1));
    assert_eq!(StreamContractChecker::check(&parts), Ok(()));
}

#[test]
fn missing_start_and_terminal_parts_are_reported() {
    let parts = vec![StreamPart::text_delta(PartId::new("0"), "x")];
    assert_eq!(
        messages(StreamContractChecker::check(&parts)),
        vec![
            "part 0: stream must start with stream-start, got text-delta",
            "part 0: text-delta for closed part `0`",
            "part 1: stream ended without finish or error",
        ]
    );
    assert_eq!(
        messages(StreamContractChecker::check(&[])),
        vec!["part 0: stream is empty"]
    );
}

#[test]
fn parts_after_terminal_and_unclosed_parts_are_reported() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: PartId::new("t"),
            provider_metadata: None,
        },
        StreamPart::ToolInputStart {
            id: ToolCallId::new("c1"),
            tool_name: "f".into(),
            provider_executed: false,
            dynamic: false,
            title: None,
            provider_metadata: None,
        },
        StreamPart::error(&ProviderError::message("x")),
        StreamPart::finish(FinishReason::stop(), Usage::totals(0, 0)),
    ];
    assert_eq!(
        messages(StreamContractChecker::check(&parts)),
        vec![
            "part 4: finish after the terminal part",
            "part 5: text part `t` never ended",
            "part 5: tool input `c1` never ended",
        ]
    );
}

#[test]
fn duplicate_starts_are_reported() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::stream_start(),
        StreamPart::ReasoningStart {
            id: PartId::new("r"),
            provider_metadata: None,
        },
        StreamPart::ReasoningStart {
            id: PartId::new("r"),
            provider_metadata: None,
        },
        StreamPart::ReasoningEnd {
            id: PartId::new("r"),
            provider_metadata: None,
        },
        StreamPart::ReasoningEnd {
            id: PartId::new("r"),
            provider_metadata: None,
        },
        StreamPart::finish(FinishReason::stop(), Usage::totals(0, 0)),
    ];
    assert_eq!(
        messages(StreamContractChecker::check(&parts)),
        vec![
            "part 1: duplicate stream-start",
            "part 3: reasoning part `r` started twice",
            "part 5: reasoning-end for closed part `r`",
        ]
    );
}

#[tokio::test]
async fn check_stream_drains_the_result() {
    let parts = text_parts(["a", "b"], Usage::totals(1, 1));
    let (collected, outcome) =
        StreamContractChecker::check_stream(simulate_stream(parts.clone())).await;
    assert_eq!(collected, parts);
    assert_eq!(outcome, Ok(()));
}

#[test]
fn closed_part_ids_cannot_be_reused() {
    let pairs = [
        (
            StreamPart::TextStart {
                id: "same".into(),
                provider_metadata: None,
            },
            StreamPart::TextEnd {
                id: "same".into(),
                provider_metadata: None,
            },
            "part 3: text part `same` started twice",
        ),
        (
            StreamPart::ReasoningStart {
                id: "same".into(),
                provider_metadata: None,
            },
            StreamPart::ReasoningEnd {
                id: "same".into(),
                provider_metadata: None,
            },
            "part 3: reasoning part `same` started twice",
        ),
        (
            StreamPart::ToolInputStart {
                id: "same".into(),
                tool_name: "tool".into(),
                provider_executed: false,
                dynamic: false,
                title: None,
                provider_metadata: None,
            },
            StreamPart::ToolInputEnd {
                id: "same".into(),
                provider_metadata: None,
            },
            "part 3: tool input `same` started twice",
        ),
    ];
    for (start, end, violation) in pairs {
        let parts = vec![
            StreamPart::stream_start(),
            start.clone(),
            end.clone(),
            StreamPart::finish(FinishReason::stop(), Usage::default()),
        ];
        assert_eq!(StreamContractChecker::check(&parts), Ok(()));
        assert_eq!(StreamContractChecker::check(&parts), Ok(()));
        let repeated = vec![
            StreamPart::stream_start(),
            start.clone(),
            end.clone(),
            start,
            end,
            StreamPart::finish(FinishReason::stop(), Usage::default()),
        ];
        assert_eq!(
            messages(StreamContractChecker::check(&repeated)),
            vec![violation]
        );
    }
}

#[test]
fn text_and_reasoning_share_the_part_id_namespace() {
    let text = (
        StreamPart::TextStart {
            id: "same".into(),
            provider_metadata: None,
        },
        StreamPart::TextEnd {
            id: "same".into(),
            provider_metadata: None,
        },
    );
    let reasoning = (
        StreamPart::ReasoningStart {
            id: "same".into(),
            provider_metadata: None,
        },
        StreamPart::ReasoningEnd {
            id: "same".into(),
            provider_metadata: None,
        },
    );
    for (first, second, violation) in [
        (
            &text,
            &reasoning,
            "part 3: reasoning part `same` started twice",
        ),
        (&reasoning, &text, "part 3: text part `same` started twice"),
    ] {
        let parts = [
            StreamPart::stream_start(),
            first.0.clone(),
            first.1.clone(),
            second.0.clone(),
            second.1.clone(),
            StreamPart::finish(FinishReason::stop(), Usage::default()),
        ];
        assert_eq!(
            messages(StreamContractChecker::check(&parts)),
            vec![violation]
        );
    }
}
