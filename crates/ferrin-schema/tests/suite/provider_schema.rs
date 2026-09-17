//! Object/default parsing needed by the pinned provider tool schemas.

use ferrin_schema::Schema;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn defaults_and_unknown_fields_follow_provider_parser_behavior() {
    let schema = Schema::from_provider_json_schema(json!({
        "type":"object","properties":{
            "type":{"const":"code_execution_result"},"stdout":{"type":"string"},
            "content":{"type":"array","items":{"type":"object","properties":{"file_id":{"type":"string"}},"required":["file_id"]},"default":[]}
        },"required":["type","stdout"]
    }));
    assert_eq!(
        schema
            .validate(json!({"type":"code_execution_result","stdout":"ok","unused":42}))
            .unwrap(),
        json!({"type":"code_execution_result","stdout":"ok","content":[]})
    );
    assert!(
        schema
            .validate(json!({"type":"code_execution_result"}))
            .is_err()
    );
    assert_eq!(schema.validate(json!({"type":"code_execution_result","stdout":"ok","content":[{"file_id":"f","extra":"strip"}]})).unwrap(),json!({"type":"code_execution_result","stdout":"ok","content":[{"file_id":"f"}]}));
}

#[test]
fn union_branches_strip_only_the_selected_variant_and_keep_records() {
    let schema = Schema::from_provider_json_schema(json!({"anyOf":[
        {"type":"object","properties":{"type":{"const":"exit"},"exitCode":{"type":"number"}},"required":["type","exitCode"]},
        {"type":"object","properties":{"type":{"const":"timeout"},"metadata":{"type":"object","additionalProperties":{}}},"required":["type"]}
    ]}));
    assert_eq!(
        schema
            .validate(
                json!({"type":"timeout","exitCode":1,"metadata":{"User_Key":{"keep_me":true}}})
            )
            .unwrap(),
        json!({"type":"timeout","metadata":{"User_Key":{"keep_me":true}}})
    );
    assert!(schema.validate(json!({"type":"exit"})).is_err());
}

#[test]
fn references_tuples_strict_objects_and_intersections_retain_their_contracts() {
    let schema = Schema::from_provider_json_schema(json!({
        "$defs":{"point":{"type":"array","items":[{"type":"integer"},{"type":"integer"}],"minItems":2,"maxItems":2}},
        "allOf":[
            {"type":"object","properties":{"point":{"$ref":"#/$defs/point"}},"required":["point"]},
            {"type":"object","properties":{"metadata":{"type":"object","additionalProperties":{}}},"required":["metadata"]}
        ]
    }));
    assert_eq!(
        schema
            .validate(json!({"point":[1,2],"metadata":{"preserved":null},"extra":true}))
            .unwrap(),
        json!({"point":[1,2],"metadata":{"preserved":null}})
    );
    assert!(
        schema
            .validate(json!({"point":[1,2,3],"metadata":{}}))
            .is_err()
    );
    let strict = Schema::from_provider_json_schema(
        json!({"type":"object","properties":{},"additionalProperties":false}),
    );
    assert!(strict.validate(json!({"extra":1})).is_err());
    assert_eq!(
        Schema::from_provider_json_schema(json!({}))
            .validate(json!({"anything":[null,true]}))
            .unwrap(),
        json!({"anything":[null,true]})
    );
}

#[test]
fn recursive_schema_and_deep_input_are_bounded() {
    let recursive = Schema::from_provider_json_schema(json!({"$ref":"#"}));
    assert!(recursive.validate(json!(null)).is_err());
    let mut deep = json!(0);
    for _ in 0..130 {
        deep = json!([deep]);
    }
    assert!(
        Schema::from_provider_json_schema(json!({}))
            .validate(deep)
            .is_err()
    );
}
