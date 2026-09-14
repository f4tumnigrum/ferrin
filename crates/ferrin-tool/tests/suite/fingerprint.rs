use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolDrift;
use ferrin_tool::ToolSet;
use ferrin_tool::fingerprint::canonical_json;
use ferrin_tool::fingerprint::detect_tool_drift;
use ferrin_tool::fingerprint::fingerprint_tools;
use ferrin_tool::fingerprint::hash_canonical;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn canonical_json_sorts_keys() {
    let value = json!({ "b": [1, { "z": null, "a": "x" }], "a": true });
    assert_eq!(
        canonical_json(&value),
        "{\"a\":true,\"b\":[1,{\"a\":\"x\",\"z\":null}]}"
    );
    let reordered = json!({ "a": true, "b": [1, { "a": "x", "z": null }] });
    assert_eq!(hash_canonical(&value), hash_canonical(&reordered));
    assert_eq!(hash_canonical(&json!("")).len(), 43);
}

#[test]
fn fingerprints_detect_changes() {
    let schema = Schema::from_json_schema(
        json!({ "type": "object", "properties": { "q": { "type": "string" } } }),
    );
    let baseline = ToolSet::new()
        .insert(
            "search",
            Tool::dynamic(schema.clone()).description("Search").build(),
        )
        .unwrap()
        .insert("old", Tool::dynamic(schema.clone()).build())
        .unwrap();
    let current = ToolSet::new()
        .insert(
            "search",
            Tool::dynamic(schema.clone())
                .description("Search the web")
                .build(),
        )
        .unwrap()
        .insert("new", Tool::dynamic(schema.clone()).build())
        .unwrap();
    let before = fingerprint_tools(&baseline);
    let after = fingerprint_tools(&current);
    assert_eq!(
        detect_tool_drift(&after, &before),
        ToolDrift {
            added: vec!["new".into()],
            removed: vec!["old".into()],
            changed: vec!["search".into()],
        }
    );
    assert!(detect_tool_drift(&before, &before).is_empty());

    let dynamic_a = ToolSet::new()
        .insert(
            "t",
            Tool::dynamic(schema.clone())
                .description_fn(|_| async { "a".to_owned() })
                .build(),
        )
        .unwrap();
    let dynamic_b = ToolSet::new()
        .insert(
            "t",
            Tool::dynamic(schema)
                .description_fn(|_| async { "b".to_owned() })
                .build(),
        )
        .unwrap();
    assert_eq!(fingerprint_tools(&dynamic_a), fingerprint_tools(&dynamic_b));
}
