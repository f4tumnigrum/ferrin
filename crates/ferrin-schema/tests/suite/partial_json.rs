use ferrin_schema::PartialParseState;
use ferrin_schema::partial_json::parse_partial;
use ferrin_schema::partial_json::repair;
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use serde_json::Value;
use serde_json::json;

#[test]
fn repairs_truncated_documents() {
    let cases = [
        ("", ""),
        ("{", "{}"),
        ("[", "[]"),
        ("{\"a\"", "{}"),
        ("{\"a\":", "{}"),
        ("{\"a\": 1", "{\"a\": 1}"),
        ("{\"a\": 1,", "{\"a\": 1}"),
        ("{\"a\": 1, \"b", "{\"a\": 1}"),
        ("{\"a\": \"hel", "{\"a\": \"hel\"}"),
        ("{\"a\": \"he\\", "{\"a\": \"he\"}"),
        ("{\"a\": \"he\\u12", "{\"a\": \"he\"}"),
        ("{\"a\": \"he\\u1234", "{\"a\": \"he\\u1234\"}"),
        ("{\"a\": [tru", "{\"a\": [true]}"),
        ("{\"a\": nul", "{\"a\": null}"),
        ("{\"a\": fa", "{\"a\": false}"),
        ("[-", "[]"),
        ("{\"a\": -", "{}"),
        ("[1, 2,", "[1, 2]"),
        ("[1, -", "[1]"),
        ("[1.", "[1]"),
        ("[1e", "[1]"),
        ("[1e5", "[1e5]"),
        ("-", ""),
        ("t", "true"),
        (
            "{\"a\": {\"b\": [\"x\", {\"c\": 12",
            "{\"a\": {\"b\": [\"x\", {\"c\": 12}]}}",
        ),
        ("{\"a\": \"日本", "{\"a\": \"日本\"}"),
        ("{\"a\": \"日", "{\"a\": \"日\"}"),
        ("[\"😀", "[\"😀\"]"),
    ];
    for (input, expected) in cases {
        assert_eq!(repair(input), expected, "input {input:?}");
    }
    assert!(matches!(
        repair("{\"a\": 1}"),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn parse_partial_reports_state() {
    let complete = parse_partial("{\"a\": [1, 2]}");
    assert_eq!(complete.state, PartialParseState::SuccessfulParse);
    assert_eq!(complete.value, Some(json!({ "a": [1, 2] })));

    let repaired = parse_partial("{\"a\": [1, 2");
    assert_eq!(repaired.state, PartialParseState::RepairedParse);
    assert_eq!(repaired.value, Some(json!({ "a": [1, 2] })));

    let failed = parse_partial("");
    assert_eq!(failed.state, PartialParseState::FailedParse);
    assert_eq!(failed.value, None);
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        (-1.0e6f64..1.0e6).prop_map(|f| json!(f)),
        "[ -~]{0,8}".prop_map(Value::String),
        "\\PC{0,6}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec(("[a-z ]{0,5}", inner), 0..4)
                .prop_map(|entries| { Value::Object(entries.into_iter().collect()) }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_prefix_of_valid_json_repairs_to_valid_json(value in json_value(), pretty in any::<bool>()) {
        let text = if pretty {
            serde_json::to_string_pretty(&value).unwrap()
        } else {
            serde_json::to_string(&value).unwrap()
        };
        for (index, _) in text.char_indices().skip(1) {
            let prefix = &text[..index];
            let trimmed = prefix.trim();
            if trimmed.is_empty() || trimmed == "-" {
                continue;
            }
            let repaired = repair(prefix);
            prop_assert!(
                serde_json::from_str::<Value>(&repaired).is_ok(),
                "prefix {prefix:?} repaired to unparsable {repaired:?}"
            );
        }
        let full = repair(&text);
        prop_assert_eq!(serde_json::from_str::<Value>(&full).unwrap(), value);
    }
}
