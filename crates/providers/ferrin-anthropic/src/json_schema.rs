//! JSON Schema sanitization for Anthropic structured outputs and strict
//! tools.
//!
//! Anthropic accepts a JSON Schema subset. Supported keywords are kept,
//! `oneOf` becomes `anyOf`, objects get `additionalProperties: false`, and
//! unsupported constraints are appended to the `description` so the model
//! still sees them.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;

const SUPPORTED_STRING_FORMATS: &[&str] = &[
    "date-time",
    "time",
    "date",
    "duration",
    "email",
    "hostname",
    "uri",
    "ipv4",
    "ipv6",
    "uuid",
];

const CONSTRAINT_KEYWORDS: &[&str] = &[
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "pattern",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minProperties",
    "maxProperties",
    "not",
];

const PASSTHROUGH_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "default",
    "const",
    "enum",
    "type",
];

/// Sanitizes `schema` into the subset accepted by Anthropic.
#[must_use]
pub fn sanitize_json_schema(schema: &JsonValue) -> JsonValue {
    let Some(object) = schema.as_object() else {
        return schema.clone();
    };
    if let Some(reference) = object.get("$ref") {
        let mut out = JsonObject::new();
        out.insert("$ref".to_owned(), reference.clone());
        return JsonValue::Object(out);
    }
    let mut out = JsonObject::new();
    for key in PASSTHROUGH_KEYWORDS {
        if let Some(value) = object.get(*key) {
            out.insert((*key).to_owned(), value.clone());
        }
    }
    let mut unsupported: Vec<(String, JsonValue)> = Vec::new();
    for key in CONSTRAINT_KEYWORDS {
        if let Some(value) = object.get(*key)
            && !value.is_null()
            && *value != JsonValue::Bool(false)
        {
            unsupported.push(((*key).to_owned(), value.clone()));
        }
    }
    if let Some(format) = object.get("format").and_then(JsonValue::as_str) {
        if SUPPORTED_STRING_FORMATS.contains(&format) {
            out.insert("format".to_owned(), JsonValue::from(format));
        } else {
            unsupported.push(("format".to_owned(), JsonValue::from(format)));
        }
    }
    if let Some(any_of) = object.get("anyOf").or_else(|| object.get("oneOf")) {
        out.insert("anyOf".to_owned(), sanitize_list(any_of));
    }
    if let Some(all_of) = object.get("allOf") {
        out.insert("allOf".to_owned(), sanitize_list(all_of));
    }
    for key in ["definitions", "$defs"] {
        if let Some(defs) = object.get(key) {
            out.insert(key.to_owned(), sanitize_record(defs));
        }
    }
    let is_object = object.get("type").and_then(JsonValue::as_str) == Some("object")
        || object.get("properties").is_some();
    if is_object {
        if let Some(properties) = object.get("properties") {
            out.insert("properties".to_owned(), sanitize_record(properties));
        }
        out.insert("additionalProperties".to_owned(), JsonValue::Bool(false));
        if let Some(required) = object.get("required") {
            out.insert("required".to_owned(), required.clone());
        }
    }
    if let Some(items) = object.get("items") {
        let items = if items.is_array() {
            sanitize_list(items)
        } else {
            sanitize_json_schema(items)
        };
        out.insert("items".to_owned(), items);
    }
    if !unsupported.is_empty() {
        let rendered = unsupported
            .iter()
            .map(|(name, value)| format!("{}: {}", humanize(name), render_value(value)))
            .collect::<Vec<_>>()
            .join("; ");
        let description = match out.get("description").and_then(JsonValue::as_str) {
            Some(existing) => format!("{existing}\n{rendered}."),
            None => format!("{rendered}."),
        };
        out.insert("description".to_owned(), JsonValue::from(description));
    }
    JsonValue::Object(out)
}

fn sanitize_list(value: &JsonValue) -> JsonValue {
    match value.as_array() {
        Some(list) => JsonValue::Array(list.iter().map(sanitize_json_schema).collect()),
        None => value.clone(),
    }
}

fn sanitize_record(value: &JsonValue) -> JsonValue {
    match value.as_object() {
        Some(record) => JsonValue::Object(
            record
                .iter()
                .map(|(name, schema)| (name.clone(), sanitize_json_schema(schema)))
                .collect(),
        ),
        None => value.clone(),
    }
}

/// `exclusiveMinimum` → `exclusive minimum`.
fn humanize(keyword: &str) -> String {
    let mut out = String::with_capacity(keyword.len() + 4);
    for c in keyword.chars() {
        if c.is_ascii_uppercase() {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn render_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(text) => text.clone(),
        other => other.to_string(),
    }
}
