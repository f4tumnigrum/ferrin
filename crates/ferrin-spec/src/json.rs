//! JSON value aliases shared by the whole specification.
//!
//! Ferrin uses `serde_json` types directly instead of defining its own JSON
//! model. The aliases exist so that the specification reads uniformly and so
//! that a future change of representation touches one place.

/// A JSON value.
pub type JsonValue = serde_json::Value;

/// A JSON object (string keys, insertion order preserved).
pub type JsonObject = serde_json::Map<String, JsonValue>;

/// Returns `true` when an optional JSON object is `None` or empty.
///
/// Used with `#[serde(skip_serializing_if = "...")]` so that empty option
/// maps do not appear in serialized output.
#[must_use]
pub fn is_none_or_empty_object(value: &Option<JsonObject>) -> bool {
    value.as_ref().is_none_or(JsonObject::is_empty)
}
