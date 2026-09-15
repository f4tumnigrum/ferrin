//! Preserve local JSON Pointer targets when strict nullability moves schemas.

use std::collections::HashMap;

use serde_json::Map;
use serde_json::Value;
use url::Url;

#[derive(Clone, Copy)]
enum IdKeyword {
    Modern,
    Draft04,
}

pub(crate) fn child_path(parent: &str, name: &str) -> String {
    let escaped = name.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

/// Visits schema positions without interpreting defaults or examples as schemas.
pub(crate) fn visit_children(
    object: &mut Map<String, Value>,
    path: &str,
    visit: &mut dyn FnMut(&mut Value, &str),
) {
    for key in [
        "properties",
        "patternProperties",
        "definitions",
        "$defs",
        "dependentSchemas",
        "dependencies",
    ] {
        if let Some(Value::Object(children)) = object.get_mut(key) {
            let parent = child_path(path, key);
            for (name, child) in children {
                // Draft-07 property dependencies are arrays of names, not schemas.
                if !child.is_array() {
                    visit(child, &child_path(&parent, name));
                }
            }
        }
    }
    for key in [
        "items",
        "additionalItems",
        "additionalProperties",
        "anyOf",
        "allOf",
        "oneOf",
        "prefixItems",
        "not",
        "if",
        "then",
        "else",
        "contains",
        "propertyNames",
        "unevaluatedItems",
        "unevaluatedProperties",
    ] {
        if let Some(child) = object.get_mut(key) {
            let parent = child_path(path, key);
            match child {
                Value::Array(children) => {
                    for (index, child) in children.iter_mut().enumerate() {
                        visit(child, &child_path(&parent, &index.to_string()));
                    }
                }
                child => visit(child, &parent),
            }
        }
    }
}

/// Moves are recorded in insertion order, in each intermediate tree's coordinates.
pub(crate) fn rewrite_local_references(schema: &mut Value, moves: &[String]) {
    if moves.is_empty() {
        return;
    }
    // An anonymous schema still has a document root; this base is never fetched.
    let Ok(base) = Url::parse("https://ferrin.invalid/schema") else {
        return;
    };
    let mut resources = HashMap::from([(base.clone(), String::new())]);
    collect_resources(schema, "", &base, IdKeyword::Modern, &mut resources);
    rewrite(schema, "", &base, IdKeyword::Modern, moves, &resources);
}

fn scope(object: &Map<String, Value>, base: &Url, inherited: IdKeyword) -> (Url, IdKeyword) {
    let id_keyword = object
        .get("$schema")
        .and_then(Value::as_str)
        .map_or(inherited, |dialect| {
            if dialect.trim_end_matches('#').ends_with("/draft-04/schema") {
                IdKeyword::Draft04
            } else {
                IdKeyword::Modern
            }
        });
    let identifier = object
        .get(match id_keyword {
            IdKeyword::Modern => "$id",
            IdKeyword::Draft04 => "id",
        })
        .and_then(Value::as_str);
    let resolved = identifier
        .and_then(|id| base.join(id).ok())
        .unwrap_or_else(|| base.clone());
    (resolved, id_keyword)
}

fn collect_resources(
    schema: &mut Value,
    path: &str,
    base: &Url,
    inherited: IdKeyword,
    resources: &mut HashMap<Url, String>,
) {
    let Value::Object(object) = schema else {
        return;
    };
    let (base, id_keyword) = scope(object, base, inherited);
    let mut resource = base.clone();
    resource.set_fragment(None);
    // Repeated fragment identifiers are anchors in the existing document.
    resources.entry(resource).or_insert_with(|| path.to_owned());
    visit_children(object, path, &mut |child, child_path| {
        collect_resources(child, child_path, &base, id_keyword, resources);
    });
}

fn rewrite(
    schema: &mut Value,
    path: &str,
    base: &Url,
    inherited: IdKeyword,
    moves: &[String],
    resources: &HashMap<Url, String>,
) {
    let Value::Object(object) = schema else {
        return;
    };
    let (base, id_keyword) = scope(object, base, inherited);
    for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
        if let Some(Value::String(reference)) = object.get_mut(keyword) {
            rewrite_reference(reference, &base, moves, resources);
        }
    }
    visit_children(object, path, &mut |child, child_path| {
        rewrite(child, child_path, &base, id_keyword, moves, resources);
    });
}

fn rewrite_reference(
    reference: &mut String,
    base: &Url,
    moves: &[String],
    resources: &HashMap<Url, String>,
) {
    let Ok(mut target_uri) = base.join(reference) else {
        return;
    };
    let Some(pointer) = target_uri.fragment().and_then(decode_fragment) else {
        return;
    };
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return;
    }
    target_uri.set_fragment(None);
    // Only resources declared in this schema are indexed; external references
    // stay untouched and no retrieval is performed.
    let Some(resource) = resources.get(&target_uri) else {
        return;
    };
    let mut original_resource = resource.clone();
    for moved in moves.iter().rev() {
        relocate(&mut original_resource, &format!("{moved}/anyOf/0"), moved);
    }
    let mut target = format!("{original_resource}{pointer}");
    for moved in moves {
        relocate(&mut target, moved, &format!("{moved}/anyOf/0"));
    }
    if let Some(relative) = target.strip_prefix(resource)
        && (relative.is_empty() || relative.starts_with('/'))
        && relative != pointer
        && let Some((prefix, _)) = reference.split_once('#')
    {
        *reference = format!("{prefix}#{}", encode_fragment(relative));
    }
}

fn relocate(path: &mut String, from: &str, to: &str) {
    if let Some(suffix) = path.strip_prefix(from)
        && (suffix.is_empty() || suffix.starts_with('/'))
    {
        *path = format!("{to}{suffix}");
    }
}

fn decode_fragment(fragment: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(fragment.len());
    let mut bytes = fragment.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).ok()
}

fn encode_fragment(fragment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in fragment.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/?".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}
