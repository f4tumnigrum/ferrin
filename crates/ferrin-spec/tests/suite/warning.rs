use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::Warning;

#[test]
fn serializes_with_kebab_case_tag() {
    let warning = Warning::unsupported_with_details("seed", "not supported by model");
    let value = serde_json::to_value(&warning).unwrap();
    assert_eq!(
        value,
        json!({ "type": "unsupported", "feature": "seed", "details": "not supported by model" })
    );

    let warning = Warning::deprecated("maxTokens", "use max_output_tokens");
    let value = serde_json::to_value(&warning).unwrap();
    assert_eq!(
        value,
        json!({ "type": "deprecated", "setting": "maxTokens", "message": "use max_output_tokens" })
    );
}

#[test]
fn omits_missing_details() {
    let warning = Warning::unsupported("top_k");
    let text = serde_json::to_string(&warning).unwrap();
    assert_eq!(text, r#"{"type":"unsupported","feature":"top_k"}"#);

    let parsed: Warning = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, warning);
}

#[test]
fn display_is_lowercase_without_period() {
    assert_eq!(Warning::unsupported("seed").to_string(), "unsupported seed");
    assert_eq!(
        Warning::compatibility("json", Some("emulated with tool call".to_owned())).to_string(),
        "compatibility for json: emulated with tool call"
    );
    assert_eq!(Warning::other("something").to_string(), "something");
}
