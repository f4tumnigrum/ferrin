use ferrin_schema::JsonSchema;
use ferrin_schema::Schema;
use ferrin_schema::SchemaDialect;
use ferrin_schema::SchemaTransform;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
struct Weather {
    /// City name.
    city: String,
    days: Option<u8>,
}

#[test]
fn derived_schema_is_draft07_with_closed_objects() {
    let schema = Schema::<Weather>::derived();
    let value = schema.json_schema();
    assert_eq!(value["$schema"], "http://json-schema.org/draft-07/schema#");
    assert_eq!(value["type"], "object");
    assert_eq!(value["additionalProperties"], false);
    assert_eq!(value["properties"]["city"]["type"], "string");
    assert_eq!(value["properties"]["city"]["description"], "City name.");
    assert_eq!(
        value["properties"]["days"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(value["required"], json!(["city"]));
}

#[test]
fn derived_schema_validates_by_deserializing() {
    let schema = Schema::<Weather>::derived();
    let weather = schema
        .validate(json!({ "city": "Paris", "days": 3 }))
        .unwrap();
    assert_eq!(
        weather,
        Weather {
            city: "Paris".to_owned(),
            days: Some(3)
        }
    );

    let error = schema.validate(json!({ "days": 3 })).unwrap_err();
    assert_eq!(error.value, json!({ "days": 3 }));
    assert!(
        error
            .to_string()
            .starts_with("type validation failed: missing field `city`")
    );
}

#[test]
fn dialect_can_be_overridden() {
    let schema = Schema::<Weather>::derived_with(SchemaDialect::Draft2020_12);
    assert_eq!(
        schema.json_schema()["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn erased_and_transformed_share_validation() {
    let schema = Schema::<Weather>::derived();
    let erased = schema.erased();
    let value = json!({ "city": "Oslo" });
    assert_eq!(erased.validate(value.clone()).unwrap(), value);
    assert!(erased.validate(json!({})).is_err());

    let strict = schema
        .transformed(SchemaTransform::openai_strict())
        .unwrap();
    assert_eq!(strict.json_schema()["required"], json!(["city", "days"]));
    assert_eq!(schema.json_schema()["required"], json!(["city"]));
    assert!(strict.validate(json!({ "city": "Oslo" })).is_ok());
}

#[test]
fn custom_validator_replaces_default() {
    let schema = Schema::<Weather>::derived().with_validator(|value| {
        Ok(Weather {
            city: value["city"].as_str().unwrap_or("unknown").to_owned(),
            days: None,
        })
    });
    let weather = schema.validate(json!({})).unwrap();
    assert_eq!(weather.city, "unknown");
}

#[test]
fn raw_json_schema_helpers() {
    let empty = Schema::<Value>::empty_object();
    assert_eq!(
        empty.json_schema(),
        &json!({ "type": "object", "properties": {}, "additionalProperties": false })
    );
    let any = Schema::<Value>::any();
    assert_eq!(any.validate(json!([1, 2])).unwrap(), json!([1, 2]));

    let debug = format!("{:?}", Schema::<Weather>::derived());
    assert!(debug.starts_with("Schema { json_schema: None"));
}

#[test]
fn typed_from_json_schema_deserializes() {
    let schema = Schema::<Weather>::typed_from_json_schema(json!({
        "type": "object",
        "properties": { "city": { "type": "string" }, "days": { "type": "integer" } },
        "required": ["city"],
    }));
    let weather = schema.validate(json!({ "city": "Rome" })).unwrap();
    assert_eq!(weather.city, "Rome");
    assert!(schema.validate(json!({ "city": 1 })).is_err());
}
