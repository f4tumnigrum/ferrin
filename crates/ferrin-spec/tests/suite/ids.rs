use std::collections::HashSet;

use pretty_assertions::assert_eq;

use ferrin_spec::ProviderId;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;

#[test]
fn serializes_transparently_as_string() {
    let id = ToolCallId::new("call_1");
    assert_eq!(serde_json::to_string(&id).unwrap(), r#""call_1""#);
    let parsed: ToolCallId = serde_json::from_str(r#""call_1""#).unwrap();
    assert_eq!(parsed, id);
}

#[test]
fn converts_from_and_to_strings() {
    let name: ToolName = "weather".into();
    assert_eq!(name.as_str(), "weather");
    assert_eq!(name.to_string(), "weather");
    assert_eq!(name, "weather");
    assert_eq!(String::from(name.clone()), "weather");
    let owned: ToolName = String::from("weather").into();
    assert_eq!(owned, name);
    assert_eq!(owned.into_string(), "weather");
}

#[test]
fn usable_as_hash_key_borrowed_as_str() {
    let mut set = HashSet::new();
    set.insert(ProviderId::new("openai"));
    assert!(set.contains("openai"));
    assert!(!set.contains("anthropic"));
}
