//! Provider schemas with reference-compatible object parsing.
//!
//! Object stripping and defaults follow the Zod input schemas used by Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); implemented independently in Rust.

use std::io;
use std::io::Write;

use serde_json::Map;
use serde_json::Value;

use crate::Schema;
use crate::TypeValidationError;
use crate::json::DEFAULT_MAX_BYTES;
use crate::json::DEFAULT_MAX_DEPTH;

#[derive(Debug, thiserror::Error)]
enum ProviderSchemaError {
    #[error("provider schema input exceeds the JSON resource limits")]
    Limit,
    #[error("provider schema contains an unsupported reference")]
    Reference,
}

impl Schema<Value> {
    /// Creates a provider-input schema that applies defaults and strips undeclared object fields.
    ///
    /// Use JSON Schema generated for Zod's input mode. Objects with explicit
    /// `additionalProperties` retain that policy: records preserve arbitrary keys,
    /// and `false` rejects unknown keys. The ordinary JSON Schema constructor keeps
    /// its validation-only behavior. Parsing uses the default JSON depth/byte limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferrin_schema::Schema;
    /// use serde_json::json;
    /// let schema = Schema::from_provider_json_schema(json!({
    ///     "type": "object", "properties": {"content": {"type": "array", "default": []}}
    /// }));
    /// assert_eq!(schema.validate(json!({"ignored": 1}))?, json!({"content": []}));
    /// # Ok::<(), ferrin_schema::TypeValidationError>(())
    /// ```
    #[must_use]
    pub fn from_provider_json_schema(schema: Value) -> Self {
        let validator = Self::from_json_schema(schema.clone());
        Self::with_json_schema_and_validator(schema.clone(), move |value| {
            check_limits(&value).map_err(|error| TypeValidationError::new(value.clone(), error))?;
            let normalized = normalize(&schema, &schema, value.clone(), 0)
                .map_err(|error| TypeValidationError::new(value, error))?;
            check_limits(&normalized)
                .map_err(|error| TypeValidationError::new(normalized.clone(), error))?;
            validator.validate(normalized)
        })
    }
}

fn check_limits(value: &Value) -> Result<(), ProviderSchemaError> {
    let mut stack = vec![(value, 0)];
    while let Some((value, depth)) = stack.pop() {
        if depth > DEFAULT_MAX_DEPTH {
            return Err(ProviderSchemaError::Limit);
        }
        match value {
            Value::Array(items) => stack.extend(items.iter().map(|value| (value, depth + 1))),
            Value::Object(items) => stack.extend(items.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    serde_json::to_writer(ByteBudget(0), value).map_err(|_| ProviderSchemaError::Limit)
}

struct ByteBudget(usize);
impl Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > DEFAULT_MAX_BYTES {
            return Err(io::Error::other("JSON size limit exceeded"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn with_definitions(root: &Value, schema: &Value) -> Value {
    let mut result = schema.clone();
    if let Some(object) = result.as_object_mut() {
        for key in ["$defs", "definitions"] {
            if let Some(definitions) = root.get(key) {
                object.entry(key).or_insert_with(|| definitions.clone());
            }
        }
    }
    result
}

fn normalize(
    root: &Value,
    schema: &Value,
    mut value: Value,
    depth: usize,
) -> Result<Value, ProviderSchemaError> {
    if depth > DEFAULT_MAX_DEPTH {
        return Err(ProviderSchemaError::Limit);
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix('#')
            .and_then(|path| root.pointer(path))
            .ok_or(ProviderSchemaError::Reference)?;
        return normalize(root, target, value, depth + 1);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            for branch in branches {
                let candidate = normalize(root, branch, value.clone(), depth + 1)?;
                if Schema::from_json_schema(with_definitions(root, branch))
                    .validate(candidate.clone())
                    .is_ok()
                {
                    value = candidate;
                    break;
                }
            }
        }
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = Map::new();
        let mut all_objects = true;
        for branch in branches {
            match normalize(root, branch, value.clone(), depth + 1)? {
                Value::Object(object) => merged.extend(object),
                _ => all_objects = false,
            }
        }
        if all_objects {
            value = Value::Object(merged);
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object)
        && let Value::Object(object) = &mut value
    {
        for (key, field_schema) in properties {
            if let Some(field) = object.remove(key) {
                object.insert(
                    key.clone(),
                    normalize(root, field_schema, field, depth + 1)?,
                );
            } else if let Some(default) = field_schema.get("default") {
                object.insert(
                    key.clone(),
                    normalize(root, field_schema, default.clone(), depth + 1)?,
                );
            }
        }
        match schema.get("additionalProperties") {
            None => object.retain(|key, _| properties.contains_key(key)),
            Some(Value::Object(additional)) => {
                let additional = Value::Object(additional.clone());
                for (_, field) in object
                    .iter_mut()
                    .filter(|(key, _)| !properties.contains_key(*key))
                {
                    *field = normalize(root, &additional, field.take(), depth + 1)?;
                }
            }
            _ => {}
        }
    } else if let (Some(additional), Value::Object(object)) = (
        schema.get("additionalProperties").filter(|v| v.is_object()),
        &mut value,
    ) {
        for field in object.values_mut() {
            *field = normalize(root, additional, field.take(), depth + 1)?;
        }
    }
    if let (Some(items), Value::Array(values)) = (schema.get("items"), &mut value) {
        for (index, field) in values.iter_mut().enumerate() {
            let item_schema = if let Some(tuple) = items.as_array() {
                tuple.get(index)
            } else {
                Some(items)
            };
            if let Some(item_schema) = item_schema {
                *field = normalize(root, item_schema, field.take(), depth + 1)?;
            }
        }
    }
    Ok(value)
}
