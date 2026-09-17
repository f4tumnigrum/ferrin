//! Cases from Vercel AI SDK 6c6c221 fix-json.test.ts (Apache-2.0; see NOTICE).

use ferrin_schema::PartialParse;
use ferrin_schema::PartialParseState;
use ferrin_schema::partial_json::parse_optional;
use ferrin_schema::partial_json::repair;
use pretty_assertions::assert_eq;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    input: String,
    expected: String,
}

#[test]
fn partial_json_matches_recorded_reference_examples() {
    for line in include_str!("../fixtures/partial-json/reference.jsonl").lines() {
        let case: Case = serde_json::from_str(line).unwrap();
        assert_eq!(repair(&case.input), case.expected, "input {:?}", case.input);
    }
}

#[test]
fn partial_json_preserves_absent_empty_null_and_repaired_input_states() {
    for (input, expected) in [
        (
            None,
            PartialParse {
                value: None,
                state: PartialParseState::UndefinedInput,
            },
        ),
        (
            Some(""),
            PartialParse {
                value: None,
                state: PartialParseState::FailedParse,
            },
        ),
        (
            Some("null"),
            PartialParse {
                value: Some(serde_json::Value::Null),
                state: PartialParseState::SuccessfulParse,
            },
        ),
        (
            Some("{\"value\":1"),
            PartialParse {
                value: Some(serde_json::json!({"value":1})),
                state: PartialParseState::RepairedParse,
            },
        ),
        (
            Some("@"),
            PartialParse {
                value: None,
                state: PartialParseState::FailedParse,
            },
        ),
    ] {
        assert_eq!(parse_optional(input), expected, "input {input:?}");
    }
}
