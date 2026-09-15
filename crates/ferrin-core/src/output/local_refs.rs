//! Preserve local JSON Schema references when moving a schema under a wrapper.

use ferrin_spec::JsonValue;

/// Relocates document-relative pointers while respecting nested resources.
pub(super) fn relocate(schema: &mut JsonValue, prefix: &str) {
    let JsonValue::Object(object) = schema else {
        return;
    };
    // A non-fragment $id creates a resource with its own reference root.
    if object
        .get("$id")
        .and_then(JsonValue::as_str)
        .is_some_and(|id| !id.split('#').next().unwrap_or_default().is_empty())
    {
        return;
    }
    for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
        if let Some(JsonValue::String(reference)) = object.get_mut(keyword) {
            if reference == "#" {
                *reference = prefix.to_owned();
            } else if let Some(pointer) = reference.strip_prefix("#/") {
                *reference = format!("{prefix}/{pointer}");
            }
        }
    }
    // Visit schema positions only: examples, defaults, const and enum contain
    // instance data whose keys must not be treated as schema keywords.
    for keyword in [
        "properties",
        "patternProperties",
        "definitions",
        "$defs",
        "dependentSchemas",
        "dependencies",
    ] {
        if let Some(JsonValue::Object(children)) = object.get_mut(keyword) {
            for child in children.values_mut() {
                relocate(child, prefix);
            }
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(JsonValue::Array(children)) = object.get_mut(keyword) {
            for child in children {
                relocate(child, prefix);
            }
        }
    }
    for keyword in [
        "items",
        "additionalItems",
        "additionalProperties",
        "contains",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
        "unevaluatedItems",
        "unevaluatedProperties",
        "contentSchema",
    ] {
        if let Some(child) = object.get_mut(keyword) {
            match child {
                JsonValue::Array(children) => {
                    for child in children {
                        relocate(child, prefix);
                    }
                }
                schema => relocate(schema, prefix),
            }
        }
    }
}
