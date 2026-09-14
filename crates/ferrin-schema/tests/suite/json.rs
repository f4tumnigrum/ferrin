use ferrin_schema::JsonSchema;
use ferrin_schema::ParseLimits;
use ferrin_schema::Schema;
use ferrin_schema::SchemaError;
use ferrin_schema::json::depth;
use ferrin_schema::json::is_parsable;
use ferrin_schema::json::parse;
use ferrin_schema::json::parse_with;
use ferrin_schema::json::parse_with_schema;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;

#[test]
fn parses_within_limits() {
    assert_eq!(parse("{\"a\": [1]}").unwrap(), json!({ "a": [1] }));
    assert!(is_parsable("[]"));
    assert!(!is_parsable("["));

    let error = parse("{").unwrap_err();
    assert_eq!(error.text, "{");
    assert!(error.to_string().starts_with("json parsing failed:"));
}

#[test]
fn enforces_size_and_depth_limits() {
    let limits = ParseLimits {
        max_depth: 2,
        max_bytes: 16,
    };
    assert!(parse_with("[[1]]", limits).is_ok());
    let deep = parse_with("[[[1]]]", limits).unwrap_err();
    assert!(
        deep.to_string()
            .contains("nesting depth exceeds the limit of 2")
    );
    let large = parse_with("[1,2,3,4,5,6,7,8,9,10]", limits).unwrap_err();
    assert!(large.to_string().contains("exceeds the limit of 16 bytes"));

    assert_eq!(depth(&json!(1)), 0);
    assert_eq!(depth(&json!([])), 1);
    assert_eq!(depth(&json!({ "a": [[1]] })), 3);
}

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
struct Point {
    x: i32,
    y: i32,
}

#[test]
fn parse_with_schema_distinguishes_errors() {
    let schema = Schema::<Point>::derived();
    assert_eq!(
        parse_with_schema("{\"x\": 1, \"y\": 2}", &schema).unwrap(),
        Point { x: 1, y: 2 }
    );
    assert!(matches!(
        parse_with_schema("{\"x\": 1", &schema),
        Err(SchemaError::JsonParse(_))
    ));
    assert!(matches!(
        parse_with_schema("{\"x\": 1}", &schema),
        Err(SchemaError::TypeValidation(_))
    ));
}
