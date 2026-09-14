//! Pure helpers: capabilities, schema conversion and the JSON accumulator.

use ferrin_google::capabilities::capabilities;
use ferrin_google::capabilities::max_thinking_tokens_gemini_2_5;
use ferrin_google::capabilities::minimum_thinking_level_gemini3;
use ferrin_google::json_accumulator::JsonAccumulator;
use ferrin_google::json_accumulator::PartialArg;
use ferrin_google::json_schema::convert_json_schema_to_openapi_schema;
use ferrin_google::json_schema::is_recursive_reference_error;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn capabilities_follow_the_model_family() {
    let flash_2_5 = capabilities("gemini-2.5-flash");
    assert!(flash_2_5.is_gemini);
    assert!(flash_2_5.is_gemini_2_5);
    assert!(flash_2_5.supports_gemini2_tools);
    assert!(!flash_2_5.uses_gemini3_features);
    assert!(!flash_2_5.is_gemma);

    let pro_3 = capabilities("gemini-3-pro-preview");
    assert!(pro_3.is_gemini);
    assert!(pro_3.uses_gemini3_features);
    assert!(pro_3.supports_gemini2_tools);
    assert!(pro_3.supports_file_search);

    let legacy = capabilities("gemini-1.5-pro");
    assert!(legacy.is_gemini);
    assert!(!legacy.supports_gemini2_tools);
    assert!(!legacy.uses_gemini3_features);

    let gemma = capabilities("gemma-3-27b-it");
    assert!(gemma.is_gemma);
    assert!(!gemma.is_gemini);

    assert_eq!(max_thinking_tokens_gemini_2_5("gemini-2.5-pro"), 32_768);
    assert_eq!(max_thinking_tokens_gemini_2_5("gemini-2.5-flash"), 24_576);
    assert_eq!(
        minimum_thinking_level_gemini3("gemini-3-pro-preview"),
        "minimal"
    );
    assert_eq!(minimum_thinking_level_gemini3("gemini-flash-latest"), "low");
    assert_eq!(minimum_thinking_level_gemini3("gemini-3.7-flash"), "low");
    assert_eq!(
        minimum_thinking_level_gemini3("gemini-3.7-flash-lite"),
        "minimal"
    );
}

#[test]
fn json_schema_conversion_inlines_definitions_and_maps_nullable_types() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "description": "Order",
        "properties": {
            "id": {"type": ["string", "null"], "description": "Id"},
            "count": {"type": "integer", "minimum": 0},
            "status": {"type": "string", "enum": ["open", "closed"]},
            "priority": {"type": "integer", "enum": [1, 2, 3]},
            "items": {"type": "array", "items": {"$ref": "#/$defs/item"}, "minItems": 1},
            "meta": {"type": "object", "properties": {}},
            "flag": true,
            "kind": {"const": "order"},
            "either": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "any": {}
        },
        "required": ["id", "count"],
        "additionalProperties": false,
        "$defs": {
            "item": {
                "type": "object",
                "properties": {"name": {"type": "string", "format": "email", "minLength": 1}},
                "required": ["name"]
            }
        }
    });
    let converted = convert_json_schema_to_openapi_schema(&schema)
        .unwrap()
        .unwrap();
    insta::assert_json_snapshot!("openapi_schema", converted);
    assert_eq!(
        convert_json_schema_to_openapi_schema(&json!({})).unwrap(),
        Some(json!({}))
    );
    assert!(
        convert_json_schema_to_openapi_schema(&json!({"type": "object", "properties": {}}))
            .unwrap()
            .is_none()
    );
}

#[test]
fn recursive_and_mixed_enum_schemas_are_rejected() {
    let recursive = json!({
        "type": "object",
        "properties": {"child": {"$ref": "#/$defs/node"}},
        "$defs": {"node": {"type": "object", "properties": {"child": {"$ref": "#/$defs/node"}}}}
    });
    let error = convert_json_schema_to_openapi_schema(&recursive).unwrap_err();
    assert!(is_recursive_reference_error(&error), "{error}");
    let mixed = json!({"type": "object", "properties": {"x": {"enum": ["a", 1]}}});
    let error = convert_json_schema_to_openapi_schema(&mixed).unwrap_err();
    assert!(!is_recursive_reference_error(&error));
    assert!(error.to_string().contains("enum"), "{error}");
}

fn partial(value: serde_json::Value) -> PartialArg {
    serde_json::from_value(value).unwrap()
}

#[test]
fn json_accumulator_streams_deltas_and_closes_open_scopes() {
    let mut accumulator = JsonAccumulator::new();
    let delta = accumulator.process(&[partial(json!({
        "jsonPath": "$.location",
        "stringValue": "Bos",
        "willContinue": true
    }))]);
    assert_eq!(delta, "{\"location\":\"Bos");
    let delta = accumulator.process(&[partial(json!({
        "jsonPath": "$.location",
        "stringValue": "ton",
        "willContinue": true
    }))]);
    assert_eq!(delta, "ton");
    // A continuation without `willContinue` leaves the string open; the
    // closing quote is emitted by the next navigation (or by `finalize`).
    let delta = accumulator.process(&[partial(json!({
        "jsonPath": "$.location",
        "stringValue": ""
    }))]);
    assert_eq!(delta, "");
    let delta = accumulator.process(&[
        partial(json!({"jsonPath": "$.units", "numberValue": 2})),
        partial(json!({"jsonPath": "$.items[0].name", "stringValue": "a"})),
        partial(json!({"jsonPath": "$.items[1].name", "stringValue": "b"})),
        partial(json!({"jsonPath": "$.flags.verbose", "boolValue": true})),
        partial(json!({"jsonPath": "$.nothing", "nullValue": null})),
    ]);
    assert_eq!(
        delta,
        "\",\"units\":2,\"items\":[{\"name\":\"a\"},{\"name\":\"b\"}],\"flags\":{\"verbose\":true},\"nothing\":null"
    );
    assert_eq!(
        accumulator.current(),
        json!({
            "location": "Boston",
            "units": 2,
            "items": [{"name": "a"}, {"name": "b"}],
            "flags": {"verbose": true},
            "nothing": null
        })
        .as_object()
        .unwrap()
    );
    let (final_json, closing) = accumulator.finalize();
    assert_eq!(closing, "}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&final_json).unwrap(),
        json!({
            "location": "Boston",
            "units": 2,
            "items": [{"name": "a"}, {"name": "b"}],
            "flags": {"verbose": true},
            "nothing": null
        })
    );
}

#[test]
fn json_accumulator_without_arguments_finalizes_to_an_empty_object() {
    let accumulator = JsonAccumulator::new();
    let (final_json, closing) = accumulator.finalize();
    assert_eq!(final_json, "{}");
    assert_eq!(closing, "{}");
}
