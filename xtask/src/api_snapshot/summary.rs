//! Public API summary of one crate, computed from rustdoc JSON.

use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use super::render::render_bounds;
use super::render::render_generics;
use super::render::render_output;
use super::render::render_path;
use super::render::render_type;

/// `docs/api/<crate>.json`.
#[derive(Debug, Serialize)]
pub(super) struct Summary {
    #[serde(rename = "crate")]
    pub(super) crate_name: String,
    pub(super) version: Option<String>,
    pub(super) format_version: u64,
    pub(super) items: Vec<Item>,
}

/// One public item at one public path.
#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Item {
    path: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attrs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    traits: Vec<String>,
}

impl Item {
    fn new(path: String, kind: &str) -> Self {
        Self {
            path,
            kind: kind.to_owned(),
            signature: None,
            source: None,
            attrs: Vec::new(),
            fields: Vec::new(),
            variants: Vec::new(),
            methods: Vec::new(),
            traits: Vec::new(),
        }
    }
}

struct Index<'a> {
    index: &'a serde_json::Map<String, Value>,
}

impl Index<'_> {
    fn item(&self, id: &Value) -> Option<&Value> {
        let id = match id {
            Value::Number(number) => number.to_string(),
            Value::String(text) => text.clone(),
            _ => return None,
        };
        self.index.get(&id)
    }

    fn items<'v>(&'v self, ids: &'v Value) -> impl Iterator<Item = &'v Value> + 'v {
        ids.as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| self.item(id))
    }
}

/// Kind and body of an item's `inner` field.
fn inner_kind(item: &Value) -> Option<(&str, &Value)> {
    match &item["inner"] {
        Value::String(kind) => Some((kind.as_str(), &Value::Null)),
        Value::Object(object) => object
            .iter()
            .next()
            .map(|(kind, body)| (kind.as_str(), body)),
        _ => None,
    }
}

fn name(item: &Value) -> &str {
    item["name"].as_str().unwrap_or("_")
}

/// Attributes worth tracking on the public surface.
fn attrs(item: &Value) -> Vec<String> {
    let mut out = BTreeSet::new();
    for attr in item["attrs"].as_array().into_iter().flatten() {
        match attr {
            Value::String(name) if name == "non_exhaustive" => {
                out.insert("non_exhaustive".to_owned());
            }
            Value::Object(object) if object.contains_key("must_use") => {
                out.insert("must_use".to_owned());
            }
            Value::Object(object) if object.contains_key("repr") => {
                out.insert("repr".to_owned());
            }
            _ => {}
        }
    }
    if !item["deprecation"].is_null() {
        out.insert("deprecated".to_owned());
    }
    out.into_iter().collect()
}

/// Builds the summary by walking public modules from the crate root.
pub(super) fn summarize(crate_name: &str, document: &Value) -> Result<Summary> {
    let mut index_values = document["index"].clone();
    fill_empty_paths(&mut index_values, &document["paths"]);
    let index = Index {
        index: index_values
            .as_object()
            .context("rustdoc JSON has no index")?,
    };
    let root = index
        .item(&document["root"])
        .context("rustdoc JSON root item is missing")?;
    let mut items = Vec::new();
    let mut visited: BTreeSet<(String, String)> = BTreeSet::new();
    let root_path = name(root).to_owned();
    visit_module(&index, root, &root_path, &mut items, &mut visited);
    items.sort();
    items.dedup();
    Ok(Summary {
        crate_name: crate_name.to_owned(),
        version: document["crate_version"].as_str().map(str::to_owned),
        format_version: document["format_version"].as_u64().unwrap_or_default(),
        items,
    })
}

/// rustdoc leaves `path` empty on some trait references (for example the
/// trait of a `<Self as Trait>::Item` projection); fill it from `paths`.
fn fill_empty_paths(value: &mut Value, paths: &Value) {
    match value {
        Value::Object(object) => {
            if object.get("path").and_then(Value::as_str) == Some("")
                && let Some(id) = object.get("id").map(Value::to_string)
                && let Some(name) = paths[id.as_str()]["path"]
                    .as_array()
                    .and_then(|segments| segments.last())
                    .and_then(Value::as_str)
            {
                object.insert("path".to_owned(), Value::String(name.to_owned()));
            }
            for child in object.values_mut() {
                fill_empty_paths(child, paths);
            }
        }
        Value::Array(items) => {
            for item in items {
                fill_empty_paths(item, paths);
            }
        }
        _ => {}
    }
}

fn visit_module(
    index: &Index<'_>,
    module: &Value,
    path: &str,
    items: &mut Vec<Item>,
    visited: &mut BTreeSet<(String, String)>,
) {
    if !visited.insert((path.to_owned(), module["id"].to_string())) {
        return;
    }
    for child in index.items(&module["inner"]["module"]["items"]) {
        let Some((kind, body)) = inner_kind(child) else {
            continue;
        };
        match kind {
            "module" => {
                let child_path = format!("{path}::{}", name(child));
                let mut item = Item::new(child_path.clone(), "module");
                item.attrs = attrs(child);
                items.push(item);
                visit_module(index, child, &child_path, items, visited);
            }
            "use" => visit_use(index, body, path, items, visited),
            _ => items.push(describe(
                index,
                child,
                kind,
                body,
                &format!("{path}::{}", name(child)),
            )),
        }
    }
}

/// Records a `pub use`. Re-exports of local items are described at their
/// public path (rustdoc keeps items from private modules behind the `use`);
/// re-exports from other crates are recorded with their source only.
fn visit_use(
    index: &Index<'_>,
    body: &Value,
    path: &str,
    items: &mut Vec<Item>,
    visited: &mut BTreeSet<(String, String)>,
) {
    let is_glob = body["is_glob"].as_bool().unwrap_or(false);
    let source = body["source"].as_str().unwrap_or("?");
    let target = index
        .item(&body["id"])
        .filter(|target| target["crate_id"].as_u64() == Some(0));
    match target.and_then(|target| inner_kind(target).map(|(kind, inner)| (target, kind, inner))) {
        Some((target, "module", _)) if is_glob => {
            visit_module(index, target, path, items, visited);
        }
        Some((target, "module", _)) => {
            let child_path = format!("{path}::{}", body["name"].as_str().unwrap_or("_"));
            let mut item = Item::new(child_path.clone(), "module");
            item.source = Some(source.to_owned());
            items.push(item);
            visit_module(index, target, &child_path, items, visited);
        }
        Some((target, kind, inner)) if !is_glob => {
            let child_path = format!("{path}::{}", body["name"].as_str().unwrap_or("_"));
            let mut item = describe(index, target, kind, inner, &child_path);
            item.source = Some(source.to_owned());
            items.push(item);
        }
        _ => {
            let glob = if is_glob { "::*" } else { "" };
            let mut item = Item::new(
                format!("{path}::{}", body["name"].as_str().unwrap_or("*")),
                "use",
            );
            item.source = Some(format!("{source}{glob}"));
            items.push(item);
        }
    }
}

fn describe(index: &Index<'_>, item: &Value, kind: &str, body: &Value, path: &str) -> Item {
    let mut out = Item::new(path.to_owned(), kind);
    out.attrs = attrs(item);
    match kind {
        "function" => out.signature = Some(function_signature(item, body)),
        "constant" => {
            out.signature = Some(format!("const: {}", render_type(&body["type"])));
        }
        "static" => {
            out.signature = Some(format!("static: {}", render_type(&body["type"])));
        }
        "type_alias" => {
            let (params, predicates) = render_generics(&body["generics"]);
            out.signature = Some(format!(
                "type{params} = {}{predicates}",
                render_type(&body["type"])
            ));
        }
        "struct" => {
            let (params, predicates) = render_generics(&body["generics"]);
            out.signature = Some(format!("struct{params}{predicates}"));
            out.fields = struct_fields(index, &body["kind"]);
            describe_impls(index, &body["impls"], &mut out);
        }
        "enum" => {
            let (params, predicates) = render_generics(&body["generics"]);
            out.signature = Some(format!("enum{params}{predicates}"));
            out.variants = index
                .items(&body["variants"])
                .map(|variant| describe_variant(index, variant))
                .collect();
            if body["has_stripped_variants"].as_bool().unwrap_or(false) {
                out.variants.push("/* hidden variants */".to_owned());
            }
            describe_impls(index, &body["impls"], &mut out);
        }
        "union" => {
            let (params, predicates) = render_generics(&body["generics"]);
            out.signature = Some(format!("union{params}{predicates}"));
            describe_impls(index, &body["impls"], &mut out);
        }
        "trait" => {
            let (params, predicates) = render_generics(&body["generics"]);
            let supertraits = render_bounds(&body["bounds"]);
            let mut signature = format!("trait{params}");
            if !supertraits.is_empty() {
                signature.push_str(": ");
                signature.push_str(&supertraits);
            }
            signature.push_str(&predicates);
            if !body["is_dyn_compatible"].as_bool().unwrap_or(true) {
                signature.push_str(" /* not dyn compatible */");
            }
            out.signature = Some(signature);
            out.methods = index
                .items(&body["items"])
                .filter_map(describe_assoc_item)
                .collect();
            out.methods.sort();
        }
        "macro" => out.signature = Some("macro_rules!".to_owned()),
        "proc_macro" => {
            out.signature = Some(format!(
                "proc_macro {}",
                body["kind"].as_str().unwrap_or("?")
            ));
        }
        _ => {}
    }
    out
}

fn struct_fields(index: &Index<'_>, kind: &Value) -> Vec<String> {
    if let Some(plain) = kind.get("plain") {
        let mut fields: Vec<String> = index
            .items(&plain["fields"])
            .map(|field| {
                format!(
                    "{}: {}",
                    name(field),
                    render_type(&field["inner"]["struct_field"])
                )
            })
            .collect();
        if plain["has_stripped_fields"].as_bool().unwrap_or(false) {
            fields.push("/* private fields */".to_owned());
        }
        return fields;
    }
    if let Some(tuple) = kind.get("tuple").and_then(Value::as_array) {
        return tuple
            .iter()
            .map(|field| match index.item(field) {
                Some(field) => render_type(&field["inner"]["struct_field"]),
                None => "/* private */".to_owned(),
            })
            .collect();
    }
    Vec::new()
}

fn describe_variant(index: &Index<'_>, variant: &Value) -> String {
    let kind = &variant["inner"]["variant"]["kind"];
    let mut out = name(variant).to_owned();
    if let Some(tuple) = kind.get("tuple").and_then(Value::as_array) {
        let fields: Vec<String> = tuple
            .iter()
            .map(|field| match index.item(field) {
                Some(field) => render_type(&field["inner"]["struct_field"]),
                None => "/* private */".to_owned(),
            })
            .collect();
        out.push_str(&format!("({})", fields.join(", ")));
    } else if let Some(fields) = kind.get("struct") {
        out.push_str(&format!(
            " {{ {} }}",
            struct_fields(index, &variant_struct_kind(fields)).join(", ")
        ));
    }
    out
}

/// Adapts a variant's `struct` kind to the shape [`struct_fields`] reads.
fn variant_struct_kind(fields: &Value) -> Value {
    serde_json::json!({ "plain": fields })
}

fn describe_impls(index: &Index<'_>, impls: &Value, out: &mut Item) {
    for imp in index.items(impls) {
        let body = &imp["inner"]["impl"];
        if body["is_synthetic"].as_bool().unwrap_or(false) || !body["blanket_impl"].is_null() {
            continue;
        }
        if body["trait"].is_null() {
            out.methods
                .extend(index.items(&body["items"]).filter_map(describe_assoc_item));
        } else {
            let negative = if body["is_negative"].as_bool().unwrap_or(false) {
                "!"
            } else {
                ""
            };
            let (params, predicates) = render_generics(&body["generics"]);
            out.traits.push(format!(
                "impl{params} {negative}{} for {}{predicates}",
                render_path(&body["trait"]),
                render_type(&body["for"])
            ));
        }
    }
    out.methods.sort();
    out.methods.dedup();
    out.traits.sort();
    out.traits.dedup();
}

/// Renders a trait or impl item (method, associated type or constant).
fn describe_assoc_item(item: &Value) -> Option<String> {
    let (kind, body) = inner_kind(item)?;
    match kind {
        "function" => Some(function_signature(item, body)),
        "assoc_type" => {
            let (params, predicates) = render_generics(&body["generics"]);
            let bounds = render_bounds(&body["bounds"]);
            let mut out = format!("type {}{params}", name(item));
            if !bounds.is_empty() {
                out.push_str(": ");
                out.push_str(&bounds);
            }
            if let Some(default) = body.get("type").filter(|ty| !ty.is_null()) {
                out.push_str(" = ");
                out.push_str(&render_type(default));
            }
            out.push_str(&predicates);
            Some(out)
        }
        "assoc_const" => Some(format!(
            "const {}: {}",
            name(item),
            render_type(&body["type"])
        )),
        _ => None,
    }
}

/// `[const ][async ][unsafe ]fn name<..>(..) -> ..[ where ..]`.
fn function_signature(item: &Value, body: &Value) -> String {
    let header = &body["header"];
    let mut out = String::new();
    if header["is_const"].as_bool().unwrap_or(false) {
        out.push_str("const ");
    }
    if header["is_async"].as_bool().unwrap_or(false) {
        out.push_str("async ");
    }
    if header["is_unsafe"].as_bool().unwrap_or(false) {
        out.push_str("unsafe ");
    }
    let (params, predicates) = render_generics(&body["generics"]);
    let inputs: Vec<String> = body["sig"]["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|input| {
            let input_name = input[0].as_str().unwrap_or("_");
            let ty = &input[1];
            if input_name == "self" {
                return render_self(ty);
            }
            format!("{input_name}: {}", render_type(ty))
        })
        .collect();
    out.push_str(&format!(
        "fn {}{params}({}){}{predicates}",
        name(item),
        inputs.join(", "),
        render_output(&body["sig"]["output"])
    ));
    out
}

fn render_self(ty: &Value) -> String {
    if ty.get("generic").and_then(Value::as_str) == Some("Self") {
        return "self".to_owned();
    }
    if let Some(reference) = ty.get("borrowed_ref")
        && reference["type"].get("generic").and_then(Value::as_str) == Some("Self")
    {
        let lifetime = reference["lifetime"]
            .as_str()
            .map(|lifetime| format!("{lifetime} "))
            .unwrap_or_default();
        return if reference["is_mutable"].as_bool().unwrap_or(false) {
            format!("&{lifetime}mut self")
        } else {
            format!("&{lifetime}self")
        };
    }
    format!("self: {}", render_type(ty))
}
