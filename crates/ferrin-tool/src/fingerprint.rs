//! Tool fingerprints for drift detection.
//!
//! A fingerprint pins the server-controlled, security-relevant fields of a
//! tool: its static description (a dynamic description only pins its
//! presence), its input JSON schema and its title. Capture a baseline at
//! trust time and compare later definitions with [`detect_tool_drift`].

use std::collections::BTreeMap;

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolName;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::set::ToolSet;
use crate::tool::Description;

/// Fingerprints keyed by tool name.
pub type ToolFingerprints = BTreeMap<ToolName, String>;

/// Deterministic compact JSON with object keys sorted.
#[must_use]
pub fn canonical_json(value: &JsonValue) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &JsonValue, out: &mut String) {
    match value {
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&JsonValue::String((*key).clone()).to_string());
                out.push(':');
                if let Some(child) = map.get(*key) {
                    write_canonical(child, out);
                }
            }
            out.push('}');
        }
        JsonValue::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// SHA-256 of the canonical JSON, base64url without padding.
#[must_use]
pub fn hash_canonical(value: &JsonValue) -> String {
    let digest = Sha256::digest(canonical_json(value).as_bytes());
    BASE64_URL_SAFE_NO_PAD.encode(digest)
}

/// Fingerprints every tool in the set.
#[must_use]
pub fn fingerprint_tools(tools: &ToolSet) -> ToolFingerprints {
    tools
        .iter()
        .map(|(name, tool)| {
            let description = match tool.description() {
                Some(Description::Static(text)) => json!({ "type": "string", "value": text }),
                Some(Description::Dynamic(_)) => json!({ "type": "function" }),
                #[allow(unreachable_patterns, reason = "Description is non-exhaustive")]
                Some(_) => json!({ "type": "function" }),
                None => json!({ "type": "none" }),
            };
            let mut record = serde_json::Map::new();
            record.insert("description".to_owned(), description);
            record.insert(
                "inputSchema".to_owned(),
                tool.input_schema().json_schema().clone(),
            );
            if let Some(title) = tool.title() {
                record.insert("title".to_owned(), json!(title));
            }
            (name.clone(), hash_canonical(&JsonValue::Object(record)))
        })
        .collect()
}

/// Difference between two fingerprint maps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolDrift {
    /// Present only in the current map.
    pub added: Vec<ToolName>,
    /// Present only in the baseline.
    pub removed: Vec<ToolName>,
    /// Present in both with different digests.
    pub changed: Vec<ToolName>,
}

impl ToolDrift {
    /// Returns `true` when nothing differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compares `current` against `baseline`.
#[must_use]
pub fn detect_tool_drift(current: &ToolFingerprints, baseline: &ToolFingerprints) -> ToolDrift {
    let mut drift = ToolDrift::default();
    for (name, digest) in current {
        match baseline.get(name) {
            None => drift.added.push(name.clone()),
            Some(previous) if previous != digest => drift.changed.push(name.clone()),
            Some(_) => {}
        }
    }
    for name in baseline.keys() {
        if !current.contains_key(name) {
            drift.removed.push(name.clone());
        }
    }
    drift
}
