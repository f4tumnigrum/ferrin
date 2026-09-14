//! JSON Schema normalization for OpenAI endpoints.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;

const RECORD_KEYWORDS: &[&str] = &["properties", "patternProperties", "definitions", "$defs"];
const SINGLE_KEYWORDS: &[&str] = &[
    "additionalProperties",
    "additionalItems",
    "contains",
    "not",
    "if",
    "then",
    "else",
];
const LIST_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf"];

/// Removes `propertyNames` (unsupported by OpenAI) from every level of the
/// schema, returning the normalized schema and a compatibility warning when
/// something was removed.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] when `propertyNames`
/// is not a string schema, because dropping it would change the meaning.
pub fn normalize_json_schema(
    schema: &JsonValue,
) -> Result<(JsonValue, Vec<Warning>), ProviderError> {
    let mut removed = false;
    let normalized = normalize(schema, &mut removed)?;
    let warnings = if removed {
        vec![Warning::compatibility(
            "JSON Schema propertyNames",
            Some(
                "OpenAI does not support JSON Schema propertyNames. It was removed before sending \
                 the schema, so OpenAI will not enforce property-name constraints."
                    .to_owned(),
            ),
        )]
    } else {
        Vec::new()
    };
    Ok((normalized, warnings))
}

fn normalize(schema: &JsonValue, removed: &mut bool) -> Result<JsonValue, ProviderError> {
    let Some(object) = schema.as_object() else {
        return Ok(schema.clone());
    };
    let mut out = JsonObject::new();
    for (key, value) in object {
        match key.as_str() {
            "propertyNames" => {
                let is_string_schema = value
                    .as_object()
                    .and_then(|names| names.get("type"))
                    .and_then(JsonValue::as_str)
                    == Some("string");
                if !is_string_schema {
                    return Err(UnsupportedFunctionalityError::new(
                        "JSON Schema propertyNames that does not use a string schema",
                    )
                    .into());
                }
                *removed = true;
            }
            k if RECORD_KEYWORDS.contains(&k) => {
                out.insert(key.clone(), normalize_record(value, removed)?);
            }
            k if SINGLE_KEYWORDS.contains(&k) => {
                out.insert(key.clone(), normalize(value, removed)?);
            }
            k if LIST_KEYWORDS.contains(&k) => {
                out.insert(key.clone(), normalize_list(value, removed)?);
            }
            "items" => {
                let items = if value.is_array() {
                    normalize_list(value, removed)?
                } else {
                    normalize(value, removed)?
                };
                out.insert(key.clone(), items);
            }
            "dependencies" => {
                let Some(deps) = value.as_object() else {
                    out.insert(key.clone(), value.clone());
                    continue;
                };
                let mut mapped = JsonObject::new();
                for (name, dependency) in deps {
                    let mapped_value = if dependency.is_array() {
                        dependency.clone()
                    } else {
                        normalize(dependency, removed)?
                    };
                    mapped.insert(name.clone(), mapped_value);
                }
                out.insert(key.clone(), JsonValue::Object(mapped));
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(JsonValue::Object(out))
}

fn normalize_record(value: &JsonValue, removed: &mut bool) -> Result<JsonValue, ProviderError> {
    let Some(record) = value.as_object() else {
        return Ok(value.clone());
    };
    let mut out = JsonObject::new();
    for (name, schema) in record {
        out.insert(name.clone(), normalize(schema, removed)?);
    }
    Ok(JsonValue::Object(out))
}

fn normalize_list(value: &JsonValue, removed: &mut bool) -> Result<JsonValue, ProviderError> {
    let Some(list) = value.as_array() else {
        return Ok(value.clone());
    };
    let mut out = Vec::with_capacity(list.len());
    for schema in list {
        out.push(normalize(schema, removed)?);
    }
    Ok(JsonValue::Array(out))
}
