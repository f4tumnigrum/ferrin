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
    to_openai_strict(&mut strict);

    assert_eq!(
        strict["required"],
        json!(["location", "unit", "days", "address", "shapes", "tags"])
    );
    assert_eq!(strict["additionalProperties"], false);
    assert_eq!(
        strict["properties"]["days"]["type"],
        json!(["integer", "null"])
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
        strict["properties"]["tags"]["type"],
        json!(["array", "null"])
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

    let applied = SchemaTransform::openai_strict().applied(json!({
        "properties": { "a": { "$ref": "#/definitions/X" } },
        "propertyNames": {},
    }));
    assert_eq!(
        applied,
        json!({
            "properties": { "a": { "anyOf": [{ "$ref": "#/definitions/X" }, { "type": "null" }] } },
            "required": ["a"],
            "additionalProperties": false,
        })
    );
}
