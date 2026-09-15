//! Provider-oriented JSON Schema rewrites.

use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::transform_refs::child_path;
use crate::transform_refs::rewrite_local_references;
use crate::transform_refs::visit_children;

use crate::SchemaError;

/// A rewrite applied to a JSON Schema before it is sent to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SchemaTransform {
    /// Set `additionalProperties: false` on every object schema.
    ///
    /// Applied by default to schemas derived from Rust types, so that models
    /// cannot invent properties the type does not declare.
    AdditionalPropertiesFalse,
    /// Remove `propertyNames` keywords everywhere.
    RemovePropertyNames,
    /// OpenAI strict mode: `additionalProperties: false`, no `propertyNames`,
    /// every property required, previously optional properties nullable.
    OpenAiStrict,
}

impl SchemaTransform {
    /// The OpenAI strict-mode transform.
    #[must_use]
    pub fn openai_strict() -> Self {
        Self::OpenAiStrict
    }

    /// The `additionalProperties: false` transform.
    #[must_use]
    pub fn additional_properties_false() -> Self {
        Self::AdditionalPropertiesFalse
    }

    /// Applies the transform in place, leaving the input unchanged on error.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::UnsupportedTransform`] for dictionaries that
    /// OpenAI strict mode cannot represent.
    pub fn apply(self, schema: &mut Value) -> Result<(), SchemaError> {
        match self {
            Self::AdditionalPropertiesFalse => add_additional_properties_false(schema),
            Self::RemovePropertyNames => remove_property_names(schema),
            Self::OpenAiStrict => return to_openai_strict(schema),
        }
        Ok(())
    }

    /// Applies the transform to an owned schema and returns it.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::UnsupportedTransform`] for dictionaries that
    /// OpenAI strict mode cannot represent.
    pub fn applied(self, mut schema: Value) -> Result<Value, SchemaError> {
        self.apply(&mut schema)?;
        Ok(schema)
    }
}

/// Sets `additionalProperties: false` on every object schema that does not
/// already declare a schema for additional properties.
///
/// Objects are recognized by `type: "object"` (or a type array containing
/// `object`). Recurses into `properties`, `additionalProperties`, `items`,
/// `anyOf`, `allOf`, `oneOf` and `definitions`/`$defs`.
pub fn add_additional_properties_false(schema: &mut Value) {
    let Value::Object(obj) = schema else { return };
    if type_includes(obj, "object") {
        match obj.get_mut("additionalProperties") {
            Some(nested @ Value::Object(_)) => add_additional_properties_false(nested),
            _ => {
                obj.insert("additionalProperties".to_owned(), Value::Bool(false));
            }
        }
        if let Some(Value::Object(properties)) = obj.get_mut("properties") {
            for property in properties.values_mut() {
                add_additional_properties_false(property);
            }
        }
    }
    for_each_subschema(obj, &mut add_additional_properties_false);
}

/// Removes `propertyNames` from every schema.
pub fn remove_property_names(schema: &mut Value) {
    let Value::Object(obj) = schema else { return };
    obj.remove("propertyNames");
    if let Some(Value::Object(properties)) = obj.get_mut("properties") {
        for property in properties.values_mut() {
            remove_property_names(property);
        }
    }
    if let Some(nested @ Value::Object(_)) = obj.get_mut("additionalProperties") {
        remove_property_names(nested);
    }
    for_each_subschema(obj, &mut remove_property_names);
}

/// Rewrites a schema for OpenAI strict mode.
///
/// Every object gets `additionalProperties: false`, `propertyNames` is
/// removed, every property is listed in `required`, and properties that were
/// optional become nullable (the complete schema is wrapped in
/// `anyOf: [original, {type: null}]`). Objects are recognized by `type` or by
/// the presence of `properties`.
///
/// # Errors
///
/// Returns [`SchemaError::UnsupportedTransform`] for schema-valued or
/// explicitly true `additionalProperties`, or `patternProperties`. The input
/// is unchanged on error; arbitrary dictionary values are never discarded.
pub fn to_openai_strict(schema: &mut Value) -> Result<(), SchemaError> {
    validate_strict_maps(schema)?;
    let mut moves = Vec::new();
    rewrite_openai_strict(schema, "", &mut moves);
    rewrite_local_references(schema, &moves);
    Ok(())
}

fn validate_strict_maps(schema: &Value) -> Result<(), SchemaError> {
    let Value::Object(obj) = schema else {
        return Ok(());
    };
    for keyword in ["additionalProperties", "patternProperties"] {
        let unsupported = matches!(obj.get(keyword), Some(Value::Object(_) | Value::Bool(true)));
        if unsupported {
            return Err(SchemaError::UnsupportedTransform {
                transform: "openai strict",
                keyword,
            });
        }
    }
    for key in [
        "properties",
        "definitions",
        "$defs",
        "dependentSchemas",
        "dependencies",
    ] {
        if let Some(Value::Object(map)) = obj.get(key) {
            for child in map.values() {
                validate_strict_maps(child)?;
            }
        }
    }
    for key in [
        "items",
        "additionalItems",
        "anyOf",
        "allOf",
        "oneOf",
        "prefixItems",
        "not",
        "if",
        "then",
        "else",
        "contains",
        "unevaluatedItems",
        "unevaluatedProperties",
    ] {
        if let Some(child) = obj.get(key) {
            match child {
                Value::Array(children) => {
                    for child in children {
                        validate_strict_maps(child)?;
                    }
                }
                child => validate_strict_maps(child)?,
            }
        }
    }
    Ok(())
}

fn rewrite_openai_strict(schema: &mut Value, path: &str, moves: &mut Vec<String>) {
    let Value::Object(obj) = schema else { return };
    obj.remove("propertyNames");
    let is_object = match obj.get("type") {
        Some(Value::String(kind)) => kind == "object",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
        _ => obj.contains_key("properties"),
    };
    if is_object {
        let required: Vec<String> = obj
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(Value::Object(properties)) = obj.get_mut("properties") {
            let names: Vec<String> = properties.keys().cloned().collect();
            for (name, property) in properties.iter_mut() {
                if !required.contains(name) {
                    moves.push(child_path(&child_path(path, "properties"), name));
                    make_nullable(property);
                }
            }
            obj.insert("required".to_owned(), json!(names));
        }
        obj.insert("additionalProperties".to_owned(), Value::Bool(false));
    }
    visit_children(obj, path, &mut |child, child_path| {
        rewrite_openai_strict(child, child_path, moves);
    });
}

/// Makes a property schema accept `null`.
fn make_nullable(property: &mut Value) {
    // Null must bypass every constraint, including enum, const, and not.
    // Widening only `type` would leave those constraints rejecting null.
    let original = std::mem::take(property);
    *property = json!({ "anyOf": [original, { "type": "null" }] });
}

fn type_includes(obj: &Map<String, Value>, kind: &str) -> bool {
    match obj.get("type") {
        Some(Value::String(actual)) => actual == kind,
        Some(Value::Array(kinds)) => kinds.iter().any(|actual| actual == kind),
        _ => false,
    }
}

/// Visits `items`, `anyOf`, `allOf`, `oneOf`, `definitions` and `$defs`.
fn for_each_subschema(obj: &mut Map<String, Value>, visit: &mut dyn FnMut(&mut Value)) {
    if let Some(items) = obj.get_mut("items") {
        match items {
            Value::Array(list) => list.iter_mut().for_each(&mut *visit),
            other => visit(other),
        }
    }
    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(Value::Array(list)) = obj.get_mut(key) {
            list.iter_mut().for_each(&mut *visit);
        }
    }
    for key in ["definitions", "$defs"] {
        if let Some(Value::Object(map)) = obj.get_mut(key) {
            map.values_mut().for_each(&mut *visit);
        }
    }
}
