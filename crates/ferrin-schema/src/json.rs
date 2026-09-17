//! JSON parsing with resource limits.

use ferrin_spec::error::JsonParseError;
use serde_json::Value;

use crate::error::SchemaError;
use crate::schema::Schema;

/// Default maximum nesting depth.
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Default maximum input size in bytes (64 MiB).
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Resource limits applied while parsing JSON text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum nesting depth of arrays and objects.
    pub max_depth: usize,
    /// Maximum input length in bytes.
    pub max_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum LimitError {
    #[error("input of {len} bytes exceeds the limit of {max} bytes")]
    TooLarge { len: usize, max: usize },
    #[error("nesting depth exceeds the limit of {max}")]
    TooDeep { max: usize },
    #[error("object contains forbidden prototype property")]
    PrototypeProperty,
}

/// Parses `text` with the default limits.
///
/// # Errors
///
/// Returns [`JsonParseError`] when the text is not valid JSON or exceeds the
/// limits.
pub fn parse(text: &str) -> Result<Value, JsonParseError> {
    parse_with(text, ParseLimits::default())
}

/// Parses `text` with explicit limits.
///
/// # Errors
///
/// Returns [`JsonParseError`] when the text is not valid JSON or exceeds the
/// limits.
pub fn parse_with(text: &str, limits: ParseLimits) -> Result<Value, JsonParseError> {
    if text.len() > limits.max_bytes {
        return Err(JsonParseError::new(
            truncate(text),
            LimitError::TooLarge {
                len: text.len(),
                max: limits.max_bytes,
            },
        ));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|error| JsonParseError::new(truncate(text), error))?;
    if depth(&value) > limits.max_depth {
        return Err(JsonParseError::new(
            truncate(text),
            LimitError::TooDeep {
                max: limits.max_depth,
            },
        ));
    }
    check_object_keys(&value).map_err(|error| JsonParseError::new(truncate(text), error))?;
    Ok(value)
}

fn check_object_keys(value: &Value) -> Result<(), LimitError> {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(fields) => {
                if fields.contains_key("__proto__")
                    || fields
                        .get("constructor")
                        .and_then(Value::as_object)
                        .is_some_and(|constructor| constructor.contains_key("prototype"))
                {
                    return Err(LimitError::PrototypeProperty);
                }
                pending.extend(fields.values());
            }
            Value::Array(values) => pending.extend(values),
            _ => {}
        }
    }
    Ok(())
}

/// Parses `text` and validates it against `schema`.
///
/// # Errors
///
/// Returns [`SchemaError::JsonParse`] when the text is not valid JSON and
/// [`SchemaError::TypeValidation`] when the value does not match the schema.
pub fn parse_with_schema<T>(text: &str, schema: &Schema<T>) -> Result<T, SchemaError> {
    let value = parse(text)?;
    Ok(schema.validate(value)?)
}

/// Returns `true` when `text` parses as JSON within the default limits.
#[must_use]
pub fn is_parsable(text: &str) -> bool {
    parse(text).is_ok()
}

/// Nesting depth of a value: scalars are 0, `[]`/`{}` are 1.
#[must_use]
pub fn depth(value: &Value) -> usize {
    let mut max = 0;
    let mut stack: Vec<(&Value, usize)> = vec![(value, 0)];
    while let Some((current, level)) = stack.pop() {
        match current {
            Value::Array(items) => {
                max = max.max(level + 1);
                stack.extend(items.iter().map(|item| (item, level + 1)));
            }
            Value::Object(map) => {
                max = max.max(level + 1);
                stack.extend(map.values().map(|item| (item, level + 1)));
            }
            _ => max = max.max(level),
        }
    }
    max
}

/// Keeps error payloads bounded: at most 4 KiB of the offending text.
fn truncate(text: &str) -> String {
    const MAX: usize = 4096;
    if text.len() <= MAX {
        return text.to_owned();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}
