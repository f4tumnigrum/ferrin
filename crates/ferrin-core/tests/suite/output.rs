use ferrin_core::Output;
use ferrin_schema::Schema;
use ferrin_schema::schemars;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseFormat;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
struct Node {
    value: u32,
    children: Vec<Node>,
}

fn response_schema<T: 'static>(output: &Output<T>) -> JsonValue {
    match output.handler().response_format() {
        Some(ResponseFormat::Json {
            schema: Some(schema),
            ..
        }) => schema,
        other => panic!("unexpected response format: {other:?}"),
    }
}

#[test]
fn array_output_preserves_recursive_derived_schema_references() {
    let output = Output::<Vec<Node>>::array();
    let schema = Schema::from_json_schema(response_schema(&output));
    let value = json!({"elements": [{"value": 1, "children": [{"value": 2, "children": []}]}]});
    assert_eq!(schema.validate(value.clone()).unwrap(), value);
    assert!(schema.validate(json!({"elements": [{"value": 1, "children": [{"value": "invalid", "children": []}]}]})).is_err());
    let nodes = Schema::<Vec<Node>>::typed_from_json_schema(json!({"type": "array"}))
        .validate(value["elements"].clone())
        .unwrap();
    assert_eq!((nodes[0].value, nodes[0].children[0].value), (1, 2));
}

#[test]
fn array_output_preserves_definitions_and_root_references() {
    for definitions in ["definitions", "$defs"] {
        let output = Output::array_with(Schema::from_json_schema(json!({
            "type": "object",
            "properties": {
                "child": {"$ref": format!("#/{definitions}/Child")},
                "next": {"anyOf": [{"$ref": "#"}, {"type": "null"}]}
            },
            "required": ["child", "next"],
            definitions: {"Child": {"type": "integer"}}
        })));
        let schema = Schema::from_json_schema(response_schema(&output));
        let value = json!({"elements": [{"child": 1, "next": {"child": 2, "next": null}}]});
        assert_eq!(schema.validate(value.clone()).unwrap(), value);
        assert!(
            schema
                .validate(json!({"elements": [{"child": "bad", "next": null}]}))
                .is_err()
        );
    }
}

#[test]
fn array_output_keeps_resource_scopes_and_literal_reference_keys() {
    let element = json!({
        "$id": "https://example.com/node",
        "type": "object",
        "properties": {"child": {"$ref": "#/definitions/Child"}},
        "definitions": {"Child": {"const": {"$ref": "#/literal"}}}
    });
    let output = Output::array_with(Schema::from_json_schema(element.clone()));
    let wrapped = response_schema(&output);
    assert_eq!(&wrapped["properties"]["elements"]["items"], &element);
    let value = json!({"elements": [{"child": {"$ref": "#/literal"}}]});
    assert_eq!(
        Schema::from_json_schema(wrapped)
            .validate(value.clone())
            .unwrap(),
        value
    );
    let output = Output::array_with(Schema::from_json_schema(
        json!({"const": {"$ref": "#/literal"}}),
    ));
    let value = json!({"elements": [{"$ref": "#/literal"}]});
    assert_eq!(
        Schema::from_json_schema(response_schema(&output))
            .validate(value.clone())
            .unwrap(),
        value
    );
}
