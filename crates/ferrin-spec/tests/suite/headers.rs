use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::Headers;

#[test]
fn merge_overrides_existing_values() {
    let mut base = Headers::new()
        .with("x-a", "1")
        .with("x-b", "base")
        .with("authorization", "Bearer secret");
    let overlay = Headers::new().with("x-b", "overlay").with("x-c", "3");

    base.merge(&overlay);

    assert_eq!(base.get_str("x-a"), Some("1"));
    assert_eq!(base.get_str("x-b"), Some("overlay"));
    assert_eq!(base.get_str("x-c"), Some("3"));
    assert_eq!(base.len(), 4);
}

#[test]
fn merge_replaces_repeated_values_as_a_group() {
    let mut base = Headers::new();
    base.as_map_mut()
        .append("x-multi", http::HeaderValue::from_static("old"));
    let mut overlay = Headers::new();
    overlay
        .as_map_mut()
        .append("x-multi", http::HeaderValue::from_static("a"));
    overlay
        .as_map_mut()
        .append("x-multi", http::HeaderValue::from_static("b"));

    base.merge(&overlay);

    let values: Vec<&str> = base
        .as_map()
        .get_all("x-multi")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(values, vec!["a", "b"]);
}

#[test]
fn merge_pairs_skips_none_and_invalid() {
    let mut headers = Headers::new();
    headers.merge_pairs([
        ("x-a", Some("1")),
        ("x-b", None),
        ("bad header", Some("x")),
        ("x-c", Some("bad\nvalue")),
    ]);
    assert_eq!(headers.len(), 1);
    assert_eq!(headers.get_str("x-a"), Some("1"));
}

#[test]
fn user_agent_suffix_appends_or_creates() {
    let headers = Headers::new().with_user_agent_suffix(["ferrin/0.1.0", "", "openai/0.1.0"]);
    assert_eq!(
        headers.get_str("user-agent"),
        Some("ferrin/0.1.0 openai/0.1.0")
    );

    let headers = Headers::new()
        .with("user-agent", "my-app/2.0")
        .with_user_agent_suffix(["ferrin/0.1.0"]);
    assert_eq!(
        headers.get_str("user-agent"),
        Some("my-app/2.0 ferrin/0.1.0")
    );
}

#[test]
fn debug_and_serialize_mask_sensitive_values() {
    let headers = Headers::new()
        .with("authorization", "Bearer secret")
        .with("x-api-key", "sk-123")
        .with("content-type", "application/json");

    let debug = format!("{headers:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("sk-123"));
    assert!(debug.contains("application/json"));

    let value = serde_json::to_value(&headers).unwrap();
    assert_eq!(
        value,
        json!({
            "authorization": "***",
            "x-api-key": "***",
            "content-type": "application/json",
        })
    );

    let masked = headers.masked();
    assert_eq!(masked.get_str("authorization"), Some("***"));
    assert_eq!(masked.get_str("content-type"), Some("application/json"));
}

#[test]
fn deserializes_from_object() {
    let headers: Headers =
        serde_json::from_value(json!({ "x-a": "1", "Content-Type": "text/plain" })).unwrap();
    assert_eq!(headers.get_str("x-a"), Some("1"));
    assert_eq!(headers.get_str("content-type"), Some("text/plain"));

    let err = serde_json::from_value::<Headers>(json!({ "bad header": "1" })).unwrap_err();
    assert!(err.to_string().contains("invalid header"));
}

#[test]
fn insert_rejects_invalid_names_and_values() {
    let mut headers = Headers::new();
    assert!(headers.insert("ok", "value").is_ok());
    assert!(headers.insert("not ok", "value").is_err());
    assert!(headers.insert("ok", "line\nbreak").is_err());
    assert!(headers.insert("x-utf8", "héllo").is_ok());
    assert_eq!(headers.get_str("x-utf8"), None);
    assert_eq!(
        headers
            .iter_str()
            .find(|(name, _)| *name == "x-utf8")
            .map(|(_, v)| v),
        Some("héllo".to_owned())
    );
}
