//! Conversion of JSON Schema (draft 7) to the OpenAPI 3.0 schema subset
//! accepted by the Gemini API (`responseSchema`, function `parameters`).
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::error::UnsupportedFunctionalityError;

/// Prefix of the functionality of a recursive-reference error.
pub const RECURSIVE_REFERENCE_PREFIX: &str = "recursive JSON Schema reference:";

/// Whether `error` reports a recursive `$ref` (callers fall back to
/// `parametersJsonSchema`).
#[must_use]
pub fn is_recursive_reference_error(error: &UnsupportedFunctionalityError) -> bool {
    error.functionality.starts_with(RECURSIVE_REFERENCE_PREFIX)
}

struct Context<'a> {
    definitions: Option<&'a JsonObject>,
    dollar_definitions: Option<&'a JsonObject>,
    resolving: Vec<String>,
}

/// Converts `schema` to an OpenAPI schema.
///
/// Returns `None` for `null` and for an empty root object schema (no
/// properties, no `additionalProperties`).
///
/// # Errors
///
/// Returns [`UnsupportedFunctionalityError`] for `$ref`s that do not point at
/// a direct child of the root `$defs`/`definitions`, for recursive `$ref`s
/// and for enums mixing value types.
pub fn convert_json_schema_to_openapi_schema(
    schema: &JsonValue,
) -> Result<Option<JsonValue>, UnsupportedFunctionalityError> {
    let root = schema.as_object();
    let mut context = Context {
        definitions: root
            .and_then(|object| object.get("definitions"))
            .and_then(JsonValue::as_object),
        dollar_definitions: root
            .and_then(|object| object.get("$defs"))
            .and_then(JsonValue::as_object),
        resolving: Vec::new(),
    };
    convert_definition(schema, true, &mut context)
}

fn convert_definition(
    schema: &JsonValue,
    is_root: bool,
    context: &mut Context<'_>,
) -> Result<Option<JsonValue>, UnsupportedFunctionalityError> {
    let object = match schema {
        JsonValue::Null => return Ok(None),
        JsonValue::Bool(_) => {
            return Ok(Some(
                serde_json::json!({"type": "boolean", "properties": {}}),
            ));
        }
        JsonValue::Object(object) => object,
        _ => return Ok(None),
    };
    if let Some(reference) = object.get("$ref").and_then(JsonValue::as_str) {
        return convert_reference(object, reference, is_root, context);
    }
    if is_empty_object_schema(object) {
        if is_root {
            return Ok(None);
        }
        let mut result = JsonObject::new();
        result.insert("type".to_owned(), JsonValue::from("object"));
        if let Some(description) = non_empty_string(object.get("description")) {
            result.insert("description".to_owned(), JsonValue::from(description));
        }
        return Ok(Some(JsonValue::Object(result)));
    }
    let mut result = JsonObject::new();
    if let Some(description) = non_empty_string(object.get("description")) {
        result.insert("description".to_owned(), JsonValue::from(description));
    }
    if let Some(required) = object.get("required").filter(|value| !value.is_null()) {
        result.insert("required".to_owned(), required.clone());
    }
    if let Some(format) = non_empty_string(object.get("format")) {
        result.insert("format".to_owned(), JsonValue::from(format));
    }
    let schema_type = object.get("type");
    match schema_type {
        Some(JsonValue::Array(types)) => {
            let has_null = types.iter().any(|value| value.as_str() == Some("null"));
            let non_null: Vec<&JsonValue> = types
                .iter()
                .filter(|value| value.as_str() != Some("null"))
                .collect();
            if non_null.is_empty() {
                result.insert("type".to_owned(), JsonValue::from("null"));
            } else {
                result.insert(
                    "anyOf".to_owned(),
                    JsonValue::Array(
                        non_null
                            .iter()
                            .map(|value| serde_json::json!({"type": (*value).clone()}))
                            .collect(),
                    ),
                );
                if has_null {
                    result.insert("nullable".to_owned(), JsonValue::Bool(true));
                }
            }
        }
        Some(JsonValue::String(type_name)) if !type_name.is_empty() => {
            result.insert("type".to_owned(), JsonValue::from(type_name.as_str()));
        }
        _ => {}
    }
    let values: Option<Vec<JsonValue>> = match object.get("enum") {
        Some(JsonValue::Array(values)) => Some(values.clone()),
        Some(_) => None,
        None => object.get("const").map(|value| vec![value.clone()]),
    };
    if let Some(values) = values {
        add_enum_to_schema(&values, schema_type, &mut result)?;
    }
    if let Some(JsonValue::Object(properties)) = object.get("properties") {
        let mut converted = JsonObject::new();
        for (key, value) in properties {
            if let Some(schema) = convert_definition(value, false, context)? {
                converted.insert(key.clone(), schema);
            }
        }
        result.insert("properties".to_owned(), JsonValue::Object(converted));
    }
    match object.get("items") {
        Some(JsonValue::Array(items)) => {
            let converted = items
                .iter()
                .map(|item| convert_definition(item, false, context).map(or_null))
                .collect::<Result<Vec<_>, _>>()?;
            result.insert("items".to_owned(), JsonValue::Array(converted));
        }
        Some(items) if !items.is_null() && items != &JsonValue::Bool(false) => {
            if let Some(converted) = convert_definition(items, false, context)? {
                result.insert("items".to_owned(), converted);
            }
        }
        _ => {}
    }
    if let Some(JsonValue::Array(all_of)) = object.get("allOf") {
        result.insert(
            "allOf".to_owned(),
            JsonValue::Array(convert_all(all_of, context)?),
        );
    }
    if let Some(JsonValue::Array(any_of)) = object.get("anyOf") {
        let is_null_schema =
            |schema: &JsonValue| schema.get("type").and_then(JsonValue::as_str) == Some("null");
        if any_of.iter().any(is_null_schema) {
            let non_null: Vec<&JsonValue> = any_of
                .iter()
                .filter(|schema| !is_null_schema(schema))
                .collect();
            if non_null.len() == 1 {
                if let Some(JsonValue::Object(converted)) =
                    convert_definition(non_null[0], false, context)?
                {
                    result.insert("nullable".to_owned(), JsonValue::Bool(true));
                    for (key, value) in converted {
                        result.insert(key, value);
                    }
                }
            } else {
                let converted = non_null
                    .iter()
                    .map(|schema| convert_definition(schema, false, context).map(or_null))
                    .collect::<Result<Vec<_>, _>>()?;
                result.insert("anyOf".to_owned(), JsonValue::Array(converted));
                result.insert("nullable".to_owned(), JsonValue::Bool(true));
            }
        } else {
            result.insert(
                "anyOf".to_owned(),
                JsonValue::Array(convert_all(any_of, context)?),
            );
        }
    }
    if let Some(JsonValue::Array(one_of)) = object.get("oneOf") {
        result.insert(
            "oneOf".to_owned(),
            JsonValue::Array(convert_all(one_of, context)?),
        );
    }
    for key in ["minLength", "minItems", "maxItems"] {
        if let Some(value) = object.get(key) {
            result.insert(key.to_owned(), value.clone());
        }
    }
    Ok(Some(JsonValue::Object(result)))
}

fn convert_all(
    schemas: &[JsonValue],
    context: &mut Context<'_>,
) -> Result<Vec<JsonValue>, UnsupportedFunctionalityError> {
    schemas
        .iter()
        .map(|schema| convert_definition(schema, false, context).map(or_null))
        .collect()
}

fn or_null(value: Option<JsonValue>) -> JsonValue {
    value.unwrap_or(JsonValue::Null)
}

fn non_empty_string(value: Option<&JsonValue>) -> Option<&str> {
    value
        .and_then(JsonValue::as_str)
        .filter(|text| !text.is_empty())
}

fn is_empty_object_schema(object: &JsonObject) -> bool {
    object.get("type").and_then(JsonValue::as_str) == Some("object")
        && object
            .get("properties")
            .and_then(JsonValue::as_object)
            .is_none_or(JsonObject::is_empty)
        && !object.get("additionalProperties").is_some_and(is_truthy)
}

fn is_truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(value) => *value,
        JsonValue::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        JsonValue::String(text) => !text.is_empty(),
        JsonValue::Array(_) | JsonValue::Object(_) => true,
    }
}

fn convert_reference(
    object: &JsonObject,
    reference: &str,
    is_root: bool,
    context: &mut Context<'_>,
) -> Result<Option<JsonValue>, UnsupportedFunctionalityError> {
    let (definition, key) = referenced_definition(reference, context)?;
    if context.resolving.iter().any(|resolving| resolving == &key) {
        return Err(UnsupportedFunctionalityError::with_message(
            format!("{RECURSIVE_REFERENCE_PREFIX} {reference}"),
            "Google schema conversion does not support recursive JSON Schema references.",
        ));
    }
    let mut sibling = object.clone();
    sibling.remove("$ref");
    let resolved = match definition {
        JsonValue::Bool(true) => JsonValue::Object(sibling),
        JsonValue::Bool(false) => JsonValue::Bool(false),
        JsonValue::Object(definition) => {
            let mut merged = definition;
            for (key, value) in sibling {
                merged.insert(key, value);
            }
            JsonValue::Object(merged)
        }
        other => other,
    };
    context.resolving.push(key);
    let converted = convert_definition(&resolved, is_root, context);
    context.resolving.pop();
    converted
}

fn unsupported_reference(reference: &str) -> UnsupportedFunctionalityError {
    UnsupportedFunctionalityError::with_message(
        format!("JSON Schema reference: {reference}"),
        "Google schema conversion only supports references to direct children of root-level $defs or definitions.",
    )
}

fn referenced_definition(
    reference: &str,
    context: &Context<'_>,
) -> Result<(JsonValue, String), UnsupportedFunctionalityError> {
    let sources = [
        ("#/$defs/", context.dollar_definitions),
        ("#/definitions/", context.definitions),
    ];
    let Some((prefix, definitions)) = sources
        .into_iter()
        .find(|(prefix, _)| reference.starts_with(prefix))
    else {
        return Err(unsupported_reference(reference));
    };
    let encoded = &reference[prefix.len()..];
    if encoded.is_empty() || encoded.contains('/') {
        return Err(unsupported_reference(reference));
    }
    let decoded = percent_decode(encoded).ok_or_else(|| unsupported_reference(reference))?;
    if decoded.contains('/') || has_invalid_tilde_escape(&decoded) {
        return Err(unsupported_reference(reference));
    }
    let Some(definitions) = definitions else {
        return Err(unsupported_reference(reference));
    };
    let name = decoded.replace("~1", "/").replace("~0", "~");
    let Some(definition) = definitions.get(&name) else {
        return Err(unsupported_reference(reference));
    };
    Ok((definition.clone(), format!("{prefix}{name}")))
}

fn has_invalid_tilde_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'~' && !matches!(bytes.get(index + 1), Some(b'0' | b'1')))
}

fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn type_allows(schema_type: Option<&JsonValue>, enum_type: &str) -> bool {
    match schema_type {
        None | Some(JsonValue::Null) => true,
        Some(JsonValue::String(name)) => name == enum_type,
        Some(JsonValue::Array(types)) => {
            types.iter().any(|value| value.as_str() == Some(enum_type))
        }
        Some(_) => false,
    }
}

fn enum_type(values: &[JsonValue], schema_type: Option<&JsonValue>) -> Option<&'static str> {
    if values.is_empty() {
        return None;
    }
    if type_allows(schema_type, "string") && values.iter().all(JsonValue::is_string) {
        return Some("string");
    }
    let all_numbers = values
        .iter()
        .all(|value| value.as_f64().is_some_and(f64::is_finite));
    if (type_allows(schema_type, "number") || type_allows(schema_type, "integer")) && all_numbers {
        if type_allows(schema_type, "number") {
            return Some("number");
        }
        if values.iter().all(|value| {
            value.as_i64().is_some()
                || value.as_u64().is_some()
                || value.as_f64().is_some_and(|number| number.fract() == 0.0)
        }) {
            return Some("integer");
        }
    }
    if type_allows(schema_type, "boolean") && values.iter().all(JsonValue::is_boolean) {
        return Some("boolean");
    }
    None
}

fn add_enum_to_schema(
    values: &[JsonValue],
    schema_type: Option<&JsonValue>,
    result: &mut JsonObject,
) -> Result<(), UnsupportedFunctionalityError> {
    let type_is_array_with_null = matches!(
        schema_type,
        Some(JsonValue::Array(types)) if types.iter().any(|value| value.as_str() == Some("null"))
    );
    let nullable = type_is_array_with_null
        || (matches!(schema_type, None | Some(JsonValue::Null))
            && values.iter().any(JsonValue::is_null));
    let enum_values: Vec<JsonValue> = if nullable {
        values
            .iter()
            .filter(|value| !value.is_null())
            .cloned()
            .collect()
    } else {
        values.to_vec()
    };
    if !values.is_empty() && values.iter().all(JsonValue::is_null) {
        let type_allows_null = match schema_type {
            None | Some(JsonValue::Null) => true,
            Some(JsonValue::String(name)) => name == "null",
            Some(JsonValue::Array(types)) => {
                types.iter().any(|value| value.as_str() == Some("null"))
            }
            Some(_) => false,
        };
        if type_allows_null {
            result.insert("type".to_owned(), JsonValue::from("null"));
            if matches!(schema_type, Some(JsonValue::Array(_))) {
                result.remove("anyOf");
            }
            return Ok(());
        }
    }
    let Some(enum_type) = enum_type(&enum_values, schema_type) else {
        return Err(UnsupportedFunctionalityError::with_message(
            "JSON Schema enum with mixed or unsupported values",
            "Google does not support this JSON Schema enum. Enum values must share one supported primitive type and match the schema type.",
        ));
    };
    result.insert("type".to_owned(), JsonValue::from(enum_type));
    if matches!(schema_type, Some(JsonValue::Array(_))) {
        result.remove("anyOf");
    }
    if nullable {
        result.insert("nullable".to_owned(), JsonValue::Bool(true));
    }
    if enum_type == "string" {
        result.insert("enum".to_owned(), JsonValue::Array(enum_values));
    } else {
        result.insert("format".to_owned(), JsonValue::from("enum"));
        result.insert(
            "enum".to_owned(),
            JsonValue::Array(
                enum_values
                    .iter()
                    .map(|value| match value {
                        JsonValue::String(text) => JsonValue::from(text.as_str()),
                        other => JsonValue::from(other.to_string()),
                    })
                    .collect(),
            ),
        );
    }
    Ok(())
}
