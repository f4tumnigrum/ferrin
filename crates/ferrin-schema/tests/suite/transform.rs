use ferrin_schema::JsonSchema;
use ferrin_schema::SchemaDialect;
use ferrin_schema::SchemaTransform;
use ferrin_schema::transform::add_additional_properties_false;
use ferrin_schema::transform::remove_property_names;
use ferrin_schema::transform::to_openai_strict;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
enum Unit {
    Celsius,
    Fahrenheit,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[allow(dead_code)]
enum Shape {
    Circle { radius: f64 },
    Rect { width: f64, height: f64 },
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct Address {
    city: String,
    zip: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct WeatherInput {
    location: String,
    unit: Option<Unit>,
    days: Option<u8>,
    address: Option<Address>,
    shapes: Vec<Shape>,
    #[serde(default)]
    tags: Vec<String>,
}

#[test]
fn additional_properties_false_matches_reference_behaviour() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "nested": { "type": "object", "properties": { "a": { "type": "string" } } },
            "list": { "type": "array", "items": { "type": "object" } },
            "either": { "anyOf": [{ "type": "object" }, { "type": "string" }] },
            "open": { "type": "object", "additionalProperties": { "type": "object" } },
        },
        "definitions": { "Def": { "type": ["object", "null"] } },
        "propertyNames": { "pattern": "^a" },
    });
    add_additional_properties_false(&mut schema);
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["nested"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["list"]["items"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["either"]["anyOf"][0]["additionalProperties"],
        false
    );
    assert!(
        schema["properties"]["either"]["anyOf"][1]
            .get("additionalProperties")
            .is_none()
    );
    assert_eq!(
        schema["properties"]["open"]["additionalProperties"],
        json!({ "type": "object", "additionalProperties": false })
    );
    assert_eq!(schema["definitions"]["Def"]["additionalProperties"], false);
    assert!(
        schema.get("propertyNames").is_some(),
        "only the strict transform removes it"
    );

    let mut schema = json!({ "type": "string", "propertyNames": {} });
    add_additional_properties_false(&mut schema);
    assert!(schema.get("additionalProperties").is_none());
    remove_property_names(&mut schema);
    assert!(schema.get("propertyNames").is_none());
}

#[test]
fn openai_strict_requires_everything_and_nullifies_optionals() {
    let mut strict = SchemaDialect::Draft07.generate::<WeatherInput>();
    to_openai_strict(&mut strict).unwrap();

    assert_eq!(
        strict["required"],
        json!(["location", "unit", "days", "address", "shapes", "tags"])
    );
    assert_eq!(strict["additionalProperties"], false);
    assert_eq!(
        strict["properties"]["days"]["anyOf"][1],
        json!({ "type": "null" })
    );
    assert_eq!(
        strict["properties"]["unit"]["anyOf"][1],
        json!({ "type": "null" })
    );
    assert_eq!(
        strict["properties"]["address"]["anyOf"][1],
        json!({ "type": "null" })
    );
    assert_eq!(
        strict["properties"]["tags"]["anyOf"][1],
        json!({ "type": "null" })
    );
    assert_eq!(
        strict["definitions"]["Address"]["additionalProperties"],
        false
    );
    assert_eq!(
        strict["definitions"]["Address"]["required"],
        json!(["city", "zip"])
    );
    for variant in strict["definitions"]["Shape"]["oneOf"].as_array().unwrap() {
        assert_eq!(variant["additionalProperties"], false);
    }

    let applied = SchemaTransform::openai_strict()
        .applied(json!({
            "properties": { "a": { "$ref": "#/definitions/X" } },
            "propertyNames": {},
        }))
        .unwrap();
    assert_eq!(
        applied,
        json!({
            "properties": { "a": { "anyOf": [{ "$ref": "#/definitions/X" }, { "type": "null" }] } },
            "required": ["a"],
            "additionalProperties": false,
        })
    );
}

#[cfg(feature = "json-schema-validation")]
#[test]
fn optional_constraints_accept_null_without_widening_non_null_values() {
    use ferrin_schema::validation::Validator;

    for (constraint, valid, invalid) in [
        (
            json!({"type": "string", "enum": ["C", "F"]}),
            json!("C"),
            json!("K"),
        ),
        (
            json!({"type": "string", "const": "C"}),
            json!("C"),
            json!("F"),
        ),
        (
            json!({"type": "integer", "not": {"enum": [null, 0]}}),
            json!(1),
            json!(0),
        ),
        (
            json!({"allOf": [{"type": "integer"}, {"minimum": 1}]}),
            json!(1),
            json!(0),
        ),
    ] {
        let mut schema = json!({
            "type": "object", "properties": {"value": constraint}
        });
        to_openai_strict(&mut schema).unwrap();
        let validator = Validator::compile(&schema).unwrap();
        assert!(validator.is_valid(&json!({"value": null})), "{schema}");
        assert!(validator.is_valid(&json!({"value": valid})), "{schema}");
        assert!(!validator.is_valid(&json!({"value": invalid})), "{schema}");
        assert!(!validator.is_valid(&json!({})), "{schema}");
    }
}

#[test]
fn strict_rejects_dictionaries_without_mutating_the_input() {
    use ferrin_schema::Schema;
    use ferrin_schema::SchemaError;

    let dictionary = json!({"type": "object", "additionalProperties": {"type": "integer"}});
    for original in [
        dictionary.clone(),
        json!({"type": "object", "additionalProperties": true}),
        json!({"type": "object", "patternProperties": {"^a": {"type": "integer"}}}),
        json!({"type": "object", "properties": {"map": dictionary}}),
        json!({"$defs": {"Map": dictionary}}),
        json!({"anyOf": [{"type": "null"}, dictionary]}),
        json!({"dependencies": {"trigger": {"properties": {"bag": dictionary}}}}),
    ] {
        let mut schema = original.clone();
        assert!(matches!(
            SchemaTransform::OpenAiStrict.apply(&mut schema),
            Err(SchemaError::UnsupportedTransform { .. })
        ));
        assert_eq!(schema, original);
        assert!(
            Schema::from_json_schema(original)
                .transformed(SchemaTransform::OpenAiStrict)
                .is_err()
        );
    }
    let mut derived_default = dictionary.clone();
    add_additional_properties_false(&mut derived_default);
    assert_eq!(derived_default, dictionary);
}

#[cfg(feature = "json-schema-validation")]
#[test]
fn strict_nullability_preserves_local_reference_targets() {
    use ferrin_schema::validation::Validator;

    for (name, pointer) in [("value", "value"), ("v/~ ê", "v~1~0%20%C3%AA")] {
        let mut schema = json!({
            "type": "object",
            "properties": {
                name: {
                    "definitions": {"X": {"type": "integer"}},
                    "$ref": format!("#/properties/{pointer}/definitions/X")
                },
                "copy": {"$ref": format!("#/properties/{pointer}")}
            },
            "required": ["copy"]
        });
        let original = Validator::compile(&schema).unwrap();
        assert!(original.is_valid(&json!({name: 1, "copy": 2})));
        to_openai_strict(&mut schema).unwrap();
        let strict = Validator::compile(&schema).unwrap();
        assert!(strict.is_valid(&json!({name: 1, "copy": 2})), "{schema}");
        assert!(strict.is_valid(&json!({name: null, "copy": 2})), "{schema}");
        assert!(
            !strict.is_valid(&json!({name: "bad", "copy": 2})),
            "{schema}"
        );
        assert!(
            !strict.is_valid(&json!({name: 1, "copy": null})),
            "{schema}"
        );
    }
}

#[cfg(feature = "json-schema-validation")]
#[test]
fn strict_reference_relocation_respects_nested_resource_scopes() {
    use ferrin_schema::validation::Validator;

    let mut schema = json!({
        "$id": "https://example.com/root",
        "type": "object",
        "properties": {
            "value": {
                "$id": "item",
                "type": "object",
                "properties": {
                    "number": {
                        "definitions": {"X": {"type": "integer"}},
                        "$ref": "#/properties/number/definitions/X"
                    }
                }
            }
        }
    });
    let original = Validator::compile(&schema).unwrap();
    assert!(original.is_valid(&json!({"value": {"number": 1}})));
    to_openai_strict(&mut schema).unwrap();
    let strict = Validator::compile(&schema).unwrap();
    assert!(
        strict.is_valid(&json!({"value": {"number": 1}})),
        "{schema}"
    );
    assert!(
        strict.is_valid(&json!({"value": {"number": null}})),
        "{schema}"
    );
    assert!(strict.is_valid(&json!({"value": null})), "{schema}");
    assert!(
        !strict.is_valid(&json!({"value": {"number": "bad"}})),
        "{schema}"
    );
}

#[cfg(feature = "json-schema-validation")]
#[test]
fn reference_relocation_keeps_anchor_and_annotation_scopes() {
    use ferrin_schema::validation::Validator;

    let mut schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "target": {
                "definitions": {"X": {"type": "integer"}},
                "$ref": "#/properties/target/definitions/X"
            },
            "anchored": {"$id": "#named", "$ref": "#/properties/target/definitions/X"},
            "annotated": {"id": "just-an-annotation", "$ref": "#/properties/target/definitions/X"}
        },
        "required": ["anchored", "annotated"]
    });
    let value = json!({"target": 1, "anchored": 2, "annotated": 3});
    assert!(Validator::compile(&schema).unwrap().is_valid(&value));
    to_openai_strict(&mut schema).unwrap();
    let strict = Validator::compile(&schema).unwrap();
    assert!(strict.is_valid(&value), "{schema}");
    assert!(
        !strict.is_valid(&json!({"target": null, "anchored": "bad", "annotated": 3})),
        "{schema}"
    );
}

#[cfg(feature = "json-schema-validation")]
#[test]
fn absolute_and_relative_references_to_declared_resources_are_relocated() {
    use ferrin_schema::validation::Validator;

    let mut schema = json!({
        "$id": "https://example.com/root",
        "type": "object",
        "properties": {
            "value": {
                "definitions": {"X": {"type": "integer"}},
                "$ref": "https://example.com/root#/properties/value/definitions/X"
            },
            "resource": {
                "$id": "child",
                "type": "object",
                "properties": {
                    "number": {"definitions": {"X": {"type": "integer"}},
                        "$ref": "#/properties/number/definitions/X"}
                }
            },
            "absolute": {"$ref": "https://example.com/child#/properties/number/definitions/X"},
            "relative": {"$ref": "child#/properties/number/definitions/X"},
            "anchored": {"$id": "https://example.com/root#anchor",
                "$ref": "#/properties/value/definitions/X"}
        },
        "required": ["absolute", "relative", "anchored"]
    });
    let value =
        json!({"value": 1, "resource": {"number": 2}, "absolute": 3, "relative": 4, "anchored": 5});
    assert!(Validator::compile(&schema).unwrap().is_valid(&value));
    to_openai_strict(&mut schema).unwrap();
    let strict = Validator::compile(&schema).unwrap();
    assert!(strict.is_valid(&value), "{schema}");
    assert!(!strict.is_valid(&json!({"value": 1, "resource": {"number": 2}, "absolute": "bad", "relative": 4, "anchored": 5})), "{schema}");
}
