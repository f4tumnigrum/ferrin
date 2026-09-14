//! Incremental reconstruction of function call arguments streamed as
//! `partialArgs` (JSON path plus scalar value) into JSON text deltas.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Deserializer;

/// One streamed argument fragment (`functionCall.partialArgs[]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialArg {
    /// JSON path of the value (`$.location`, `$.items[0].name`).
    pub json_path: String,
    /// String value or string continuation.
    #[serde(default)]
    pub string_value: Option<String>,
    /// Number value.
    #[serde(default)]
    pub number_value: Option<serde_json::Number>,
    /// Boolean value.
    #[serde(default)]
    pub bool_value: Option<bool>,
    /// Whether the `nullValue` key was present.
    #[serde(default, deserialize_with = "present")]
    pub null_value: bool,
    /// Whether the string value continues in a later fragment.
    #[serde(default)]
    pub will_continue: Option<bool>,
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    JsonValue::deserialize(deserializer).map(|_| true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug)]
struct StackEntry {
    segment: PathSegment,
    is_array: bool,
    child_count: usize,
}

/// Accumulates `partialArgs` into an argument object while producing the
/// JSON text deltas that spell out the same object.
#[derive(Debug, Default)]
pub struct JsonAccumulator {
    args: JsonObject,
    json_text: String,
    stack: Vec<StackEntry>,
    string_open: bool,
}

impl JsonAccumulator {
    /// Creates an empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current argument object.
    #[must_use]
    pub fn current(&self) -> &JsonObject {
        &self.args
    }

    /// Applies `partial_args` and returns the JSON text delta they produce.
    pub fn process(&mut self, partial_args: &[PartialArg]) -> String {
        let mut delta = String::new();
        for arg in partial_args {
            let raw_path = arg.json_path.strip_prefix("$.").unwrap_or(&arg.json_path);
            if raw_path.is_empty() {
                continue;
            }
            let segments = parse_path(raw_path);
            let existing = get_nested(&self.args, &segments).cloned();
            if let (Some(string_value), Some(existing)) = (&arg.string_value, &existing) {
                let escaped = escape_json_string(string_value);
                let mut joined = existing.as_str().unwrap_or_default().to_owned();
                joined.push_str(string_value);
                set_nested(&mut self.args, &segments, JsonValue::from(joined));
                delta.push_str(&escaped);
                continue;
            }
            let Some((value, json)) = resolve_value(arg) else {
                continue;
            };
            set_nested(&mut self.args, &segments, value);
            delta.push_str(&self.emit_navigation(&segments, arg, &json));
        }
        self.json_text.push_str(&delta);
        delta
    }

    /// Finishes the object: returns the final JSON text and the delta that
    /// completes the text emitted so far.
    #[must_use]
    pub fn finalize(self) -> (String, String) {
        let final_json = JsonValue::Object(self.args).to_string();
        let closing = final_json
            .get(self.json_text.len()..)
            .unwrap_or_default()
            .to_owned();
        (final_json, closing)
    }

    fn ensure_root(&mut self) -> &'static str {
        if self.stack.is_empty() {
            self.stack.push(StackEntry {
                segment: PathSegment::Key(String::new()),
                is_array: false,
                child_count: 0,
            });
            "{"
        } else {
            ""
        }
    }

    fn emit_navigation(
        &mut self,
        segments: &[PathSegment],
        arg: &PartialArg,
        json: &str,
    ) -> String {
        let mut fragment = String::new();
        if self.string_open {
            fragment.push('"');
            self.string_open = false;
        }
        fragment.push_str(self.ensure_root());
        let Some((leaf, container)) = segments.split_last() else {
            return fragment;
        };
        let common_depth = self.common_stack_depth(container);
        fragment.push_str(&self.close_down_to(common_depth));
        fragment.push_str(&self.open_down_to(container, leaf));
        fragment.push_str(&self.emit_leaf(leaf, arg, json));
        fragment
    }

    fn common_stack_depth(&self, container: &[PathSegment]) -> usize {
        let max_depth = (self.stack.len().saturating_sub(1)).min(container.len());
        let common = self
            .stack
            .iter()
            .skip(1)
            .zip(container.iter().take(max_depth))
            .take_while(|(entry, segment)| entry.segment == **segment)
            .count();
        common + 1
    }

    fn close_down_to(&mut self, depth: usize) -> String {
        let mut fragment = String::new();
        while self.stack.len() > depth {
            if let Some(entry) = self.stack.pop() {
                fragment.push(if entry.is_array { ']' } else { '}' });
            }
        }
        fragment
    }

    fn open_down_to(&mut self, container: &[PathSegment], leaf: &PathSegment) -> String {
        let mut fragment = String::new();
        let start = self.stack.len().saturating_sub(1);
        for index in start..container.len() {
            let segment = &container[index];
            if let Some(parent) = self.stack.last_mut() {
                if parent.child_count > 0 {
                    fragment.push(',');
                }
                parent.child_count += 1;
            }
            if let PathSegment::Key(key) = segment {
                fragment.push_str(&JsonValue::from(key.as_str()).to_string());
                fragment.push(':');
            }
            let child = container.get(index + 1).unwrap_or(leaf);
            let is_array = matches!(child, PathSegment::Index(_));
            fragment.push(if is_array { '[' } else { '{' });
            self.stack.push(StackEntry {
                segment: segment.clone(),
                is_array,
                child_count: 0,
            });
        }
        fragment
    }

    fn emit_leaf(&mut self, leaf: &PathSegment, arg: &PartialArg, json: &str) -> String {
        let mut fragment = String::new();
        if let Some(container) = self.stack.last_mut() {
            if container.child_count > 0 {
                fragment.push(',');
            }
            container.child_count += 1;
        }
        if let PathSegment::Key(key) = leaf {
            fragment.push_str(&JsonValue::from(key.as_str()).to_string());
            fragment.push(':');
        }
        if arg.string_value.is_some() && arg.will_continue == Some(true) {
            fragment.push_str(&json[..json.len().saturating_sub(1)]);
            self.string_open = true;
        } else {
            fragment.push_str(json);
        }
        fragment
    }
}

fn escape_json_string(text: &str) -> String {
    let quoted = JsonValue::from(text).to_string();
    quoted[1..quoted.len() - 1].to_owned()
}

fn resolve_value(arg: &PartialArg) -> Option<(JsonValue, String)> {
    if let Some(text) = &arg.string_value {
        let value = JsonValue::from(text.as_str());
        let json = value.to_string();
        return Some((value, json));
    }
    if let Some(number) = &arg.number_value {
        let value = JsonValue::Number(number.clone());
        let json = value.to_string();
        return Some((value, json));
    }
    if let Some(flag) = arg.bool_value {
        return Some((JsonValue::Bool(flag), flag.to_string()));
    }
    if arg.null_value {
        return Some((JsonValue::Null, "null".to_owned()));
    }
    None
}

fn parse_path(raw_path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    for part in raw_path.split('.') {
        match part.find('[') {
            None => segments.push(PathSegment::Key(part.to_owned())),
            Some(bracket) => {
                if bracket > 0 {
                    segments.push(PathSegment::Key(part[..bracket].to_owned()));
                }
                let mut rest = &part[bracket..];
                while let Some(start) = rest.find('[') {
                    let Some(end) = rest[start..].find(']') else {
                        break;
                    };
                    if let Ok(index) = rest[start + 1..start + end].parse::<usize>() {
                        segments.push(PathSegment::Index(index));
                    }
                    rest = &rest[start + end + 1..];
                }
            }
        }
    }
    segments
}

fn get_nested<'a>(object: &'a JsonObject, segments: &[PathSegment]) -> Option<&'a JsonValue> {
    let (first, rest) = segments.split_first()?;
    let PathSegment::Key(key) = first else {
        return None;
    };
    let mut current = object.get(key)?;
    for segment in rest {
        current = match (segment, current) {
            (PathSegment::Key(key), JsonValue::Object(object)) => object.get(key)?,
            (PathSegment::Index(index), JsonValue::Array(items)) => items.get(*index)?,
            _ => return None,
        };
    }
    Some(current)
}

fn set_nested(object: &mut JsonObject, segments: &[PathSegment], value: JsonValue) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    let PathSegment::Key(key) = first else {
        return;
    };
    let Some((last, middle)) = rest.split_last() else {
        object.insert(key.clone(), value);
        return;
    };
    let mut current = object
        .entry(key.clone())
        .or_insert_with(|| container_for(rest.first()));
    for (index, segment) in middle.iter().enumerate() {
        let next = rest.get(index + 1);
        current = child_slot(current, segment, next);
    }
    match (last, current) {
        (PathSegment::Key(key), JsonValue::Object(target)) => {
            target.insert(key.clone(), value);
        }
        (PathSegment::Index(index), JsonValue::Array(items)) => {
            while items.len() <= *index {
                items.push(JsonValue::Null);
            }
            items[*index] = value;
        }
        (segment, slot) => {
            *slot = container_for(Some(segment));
            set_nested_into(slot, segment, value);
        }
    }
}

fn set_nested_into(slot: &mut JsonValue, segment: &PathSegment, value: JsonValue) {
    match (segment, slot) {
        (PathSegment::Key(key), JsonValue::Object(target)) => {
            target.insert(key.clone(), value);
        }
        (PathSegment::Index(index), JsonValue::Array(items)) => {
            while items.len() <= *index {
                items.push(JsonValue::Null);
            }
            items[*index] = value;
        }
        _ => {}
    }
}

fn container_for(segment: Option<&PathSegment>) -> JsonValue {
    match segment {
        Some(PathSegment::Index(_)) => JsonValue::Array(Vec::new()),
        _ => JsonValue::Object(JsonObject::new()),
    }
}

fn child_slot<'a>(
    current: &'a mut JsonValue,
    segment: &PathSegment,
    next: Option<&PathSegment>,
) -> &'a mut JsonValue {
    let wanted_container = container_for(next);
    match segment {
        PathSegment::Key(key) => {
            if !current.is_object() {
                *current = JsonValue::Object(JsonObject::new());
            }
            match current {
                JsonValue::Object(object) => {
                    let slot = object.entry(key.clone()).or_insert(JsonValue::Null);
                    if slot.is_null() {
                        *slot = wanted_container;
                    }
                    slot
                }
                other => other,
            }
        }
        PathSegment::Index(index) => {
            if !current.is_array() {
                *current = JsonValue::Array(Vec::new());
            }
            match current {
                JsonValue::Array(items) => {
                    while items.len() <= *index {
                        items.push(JsonValue::Null);
                    }
                    if items[*index].is_null() {
                        items[*index] = wanted_container;
                    }
                    &mut items[*index]
                }
                other => other,
            }
        }
    }
}
