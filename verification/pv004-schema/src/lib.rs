//! PV-004: schemars 1.2 draft-07 shapes for Option/enum and an OpenAI strict
//! mode transform (port of `addAdditionalPropertiesToJsonSchema` +
//! `normalizeOpenAIJsonSchema` with the strict-mode "all required + nullable"
//! rule).

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub enum Unit {
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Shape {
    Circle { radius: f64 },
    Rect { width: f64, height: f64 },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Address {
    pub city: String,
    pub zip: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WeatherInput {
    /// Location name.
    pub location: String,
    pub unit: Option<Unit>,
    pub days: Option<u8>,
    pub address: Option<Address>,
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn draft07<T: JsonSchema>() -> Value {
    SchemaSettings::draft07().into_generator().into_root_schema_for::<T>().to_value()
}

pub fn draft2020<T: JsonSchema>() -> Value {
    SchemaSettings::draft2020_12().into_generator().into_root_schema_for::<T>().to_value()
}

/// OpenAI strict-mode transform:
/// 1. every object gets `additionalProperties: false`;
/// 2. `propertyNames` removed (OpenAI strict mode rejects it);
/// 3. every property is listed in `required`; properties that were optional
///    become nullable (`type: [T, "null"]` or `anyOf: [..., {type: null}]`).
pub fn to_openai_strict(schema: &Value) -> Value {
    let mut out = schema.clone();
    visit(&mut out);
    out
}

fn visit(value: &mut Value) {
    let Value::Object(obj) = value else { return };
    obj.remove("propertyNames");
    let is_object = match obj.get("type") {
        Some(Value::String(t)) => t == "object",
        Some(Value::Array(ts)) => ts.iter().any(|t| t == "object"),
        _ => obj.contains_key("properties"),
    };
    if is_object {
        let required: Vec<String> = obj
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        if let Some(Value::Object(props)) = obj.get_mut("properties") {
            let names: Vec<String> = props.keys().cloned().collect();
            for name in &names {
                let prop = props.get_mut(name).unwrap();
                if !required.contains(name) {
                    make_nullable(prop);
                }
                visit(prop);
            }
            obj.insert("required".into(), json!(names));
        }
        match obj.get_mut("additionalProperties") {
            Some(ap @ Value::Object(_)) => visit(ap),
            _ => {
                obj.insert("additionalProperties".into(), Value::Bool(false));
            }
        }
    }
    for key in ["items", "not", "if", "then", "else", "contains"] {
        if let Some(child) = obj.get_mut(key) {
            match child {
                Value::Array(items) => items.iter_mut().for_each(visit),
                other => visit(other),
            }
        }
    }
    for key in ["anyOf", "allOf", "oneOf", "prefixItems"] {
        if let Some(Value::Array(items)) = obj.get_mut(key) {
            items.iter_mut().for_each(visit);
        }
    }
    for key in ["definitions", "$defs", "patternProperties"] {
        if let Some(Value::Object(map)) = obj.get_mut(key) {
            map.values_mut().for_each(visit);
        }
    }
}

fn make_nullable(prop: &mut Value) {
    let Value::Object(obj) = prop else { return };
    match obj.get_mut("type") {
        Some(Value::String(t)) => {
            let t = t.clone();
            obj.insert("type".into(), json!([t, "null"]));
        }
        Some(Value::Array(ts)) => {
            if !ts.iter().any(|t| t == "null") {
                ts.push(json!("null"));
            }
        }
        _ => {
            // $ref / anyOf / enum without type: wrap.
            let inner = Value::Object(std::mem::take(obj));
            let mut wrapper = Map::new();
            wrapper.insert("anyOf".into(), json!([inner, { "type": "null" }]));
            *obj = wrapper;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_shapes() {
        println!("--- draft07 WeatherInput ---\n{}", serde_json::to_string_pretty(&draft07::<WeatherInput>()).unwrap());
        println!("--- draft2020 WeatherInput (for comparison) ---\n{}", serde_json::to_string_pretty(&draft2020::<WeatherInput>()).unwrap());
        println!("--- strict ---\n{}", serde_json::to_string_pretty(&to_openai_strict(&draft07::<WeatherInput>())).unwrap());
    }

    #[test]
    fn draft07_facts() {
        let schema = draft07::<WeatherInput>();
        assert_eq!(schema["$schema"], "http://json-schema.org/draft-07/schema#");
        // Option<primitive> -> type array with null
        assert_eq!(schema["properties"]["days"]["type"], json!(["integer", "null"]));
        // Option<enum via $ref> -> anyOf [$ref, {type: null}]
        assert!(schema["properties"]["unit"]["anyOf"].is_array(), "{}", schema["properties"]["unit"]);
        assert!(schema["properties"]["address"]["anyOf"].is_array());
        // Option fields are not required; Vec without default is required
        let required: Vec<&str> = schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, vec!["location", "shapes"]);
        // unit enum -> string enum
        assert_eq!(schema["definitions"]["Unit"]["type"], "string");
        assert_eq!(schema["definitions"]["Unit"]["enum"], json!(["Celsius", "Fahrenheit"]));
        // internally tagged enum -> oneOf of objects with required tag
        assert!(schema["definitions"]["Shape"]["oneOf"].is_array());
        // definitions live under "definitions" (draft-07), not "$defs"
        assert!(schema.get("$defs").is_none());
    }

    #[test]
    fn strict_transform_properties() {
        let strict = to_openai_strict(&draft07::<WeatherInput>());
        let required: Vec<&str> = strict["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, vec!["location", "unit", "days", "address", "shapes", "tags"]);
        assert_eq!(strict["additionalProperties"], false);
        assert_eq!(strict["properties"]["days"]["type"], json!(["integer", "null"]));
        assert_eq!(strict["properties"]["unit"]["anyOf"][1], json!({"type": "null"}));
        assert_eq!(strict["definitions"]["Address"]["additionalProperties"], false);
        assert_eq!(strict["definitions"]["Address"]["required"], json!(["city", "zip"]));
        for variant in strict["definitions"]["Shape"]["oneOf"].as_array().unwrap() {
            assert_eq!(variant["additionalProperties"], false);
        }
    }
}
