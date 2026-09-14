//! Compact textual rendering of rustdoc JSON types and generics.

use serde_json::Value;

/// Renders a rustdoc JSON `Type`.
pub(super) fn render_type(ty: &Value) -> String {
    let Some(object) = ty.as_object() else {
        return "?".to_owned();
    };
    let Some((kind, body)) = object.iter().next() else {
        return "?".to_owned();
    };
    match kind.as_str() {
        "resolved_path" => render_path(body),
        "generic" | "primitive" => body.as_str().unwrap_or("?").to_owned(),
        "borrowed_ref" => {
            let lifetime = body["lifetime"]
                .as_str()
                .map(|lifetime| format!("{lifetime} "))
                .unwrap_or_default();
            let mutable = if body["is_mutable"].as_bool().unwrap_or(false) {
                "mut "
            } else {
                ""
            };
            format!("&{lifetime}{mutable}{}", render_type(&body["type"]))
        }
        "raw_pointer" => {
            let mutable = if body["is_mutable"].as_bool().unwrap_or(false) {
                "mut"
            } else {
                "const"
            };
            format!("*{mutable} {}", render_type(&body["type"]))
        }
        "tuple" => format!("({})", render_list(body)),
        "slice" => format!("[{}]", render_type(body)),
        "array" => format!(
            "[{}; {}]",
            render_type(&body["type"]),
            body["len"].as_str().unwrap_or("?")
        ),
        "impl_trait" => format!("impl {}", render_bounds(body)),
        "dyn_trait" => {
            let mut out = format!("dyn {}", render_poly_traits(&body["traits"]));
            if let Some(lifetime) = body["lifetime"].as_str() {
                out.push_str(" + ");
                out.push_str(lifetime);
            }
            out
        }
        "qualified_path" => {
            let name = body["name"].as_str().unwrap_or("?");
            let self_type = render_type(&body["self_type"]);
            match body["trait"].as_object() {
                Some(_) => format!("<{self_type} as {}>::{name}", render_path(&body["trait"])),
                None => format!("{self_type}::{name}"),
            }
        }
        "function_pointer" => {
            let sig = &body["sig"];
            let inputs = sig["inputs"]
                .as_array()
                .map(|inputs| {
                    inputs
                        .iter()
                        .map(|input| render_type(&input[1]))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("fn({inputs}){}", render_output(&sig["output"]))
        }
        "infer" => "_".to_owned(),
        "pat" => render_type(&body["type"]),
        other => format!("?{other}"),
    }
}

/// Renders a `Path` (`name<args>`), keeping the final path segment only.
pub(super) fn render_path(path: &Value) -> String {
    let name = path["path"].as_str().unwrap_or("?");
    let name = name.rsplit("::").next().unwrap_or(name);
    format!("{name}{}", render_generic_args(&path["args"]))
}

fn render_generic_args(args: &Value) -> String {
    let Some(object) = args.as_object() else {
        return String::new();
    };
    if let Some(angle) = object.get("angle_bracketed") {
        let mut parts: Vec<String> = angle["args"]
            .as_array()
            .map(|args| args.iter().map(render_generic_arg).collect())
            .unwrap_or_default();
        if let Some(constraints) = angle["constraints"].as_array() {
            for constraint in constraints {
                let name = constraint["name"].as_str().unwrap_or("?");
                let binding = &constraint["binding"];
                if let Some(equality) = binding.get("equality") {
                    let value = equality
                        .get("type")
                        .map_or_else(|| "?".to_owned(), render_type);
                    parts.push(format!("{name} = {value}"));
                } else if let Some(bounds) = binding.get("constraint") {
                    parts.push(format!("{name}: {}", render_bounds(bounds)));
                }
            }
        }
        if parts.is_empty() {
            return String::new();
        }
        return format!("<{}>", parts.join(", "));
    }
    if let Some(paren) = object.get("parenthesized") {
        let inputs = render_list(&paren["inputs"]);
        return format!("({inputs}){}", render_output(&paren["output"]));
    }
    String::new()
}

fn render_generic_arg(arg: &Value) -> String {
    if let Some(lifetime) = arg.get("lifetime") {
        return lifetime.as_str().unwrap_or("?").to_owned();
    }
    if let Some(ty) = arg.get("type") {
        return render_type(ty);
    }
    if let Some(constant) = arg.get("const") {
        return constant["expr"].as_str().unwrap_or("?").to_owned();
    }
    if arg.as_str() == Some("infer") {
        return "_".to_owned();
    }
    "?".to_owned()
}

fn render_list(types: &Value) -> String {
    types
        .as_array()
        .map(|types| types.iter().map(render_type).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

/// Renders a function output (` -> T` or nothing).
pub(super) fn render_output(output: &Value) -> String {
    if output.is_null() {
        return String::new();
    }
    format!(" -> {}", render_type(output))
}

/// Renders `GenericBound`s joined with ` + `.
pub(super) fn render_bounds(bounds: &Value) -> String {
    bounds
        .as_array()
        .map(|bounds| {
            bounds
                .iter()
                .map(render_bound)
                .collect::<Vec<_>>()
                .join(" + ")
        })
        .unwrap_or_default()
}

fn render_bound(bound: &Value) -> String {
    if let Some(trait_bound) = bound.get("trait_bound") {
        let modifier = match trait_bound["modifier"].as_str() {
            Some("maybe") => "?",
            Some("maybe_const") => "~const ",
            _ => "",
        };
        let hrtb = render_hrtb(&trait_bound["generic_params"]);
        return format!("{hrtb}{modifier}{}", render_path(&trait_bound["trait"]));
    }
    if let Some(outlives) = bound.get("outlives") {
        return outlives.as_str().unwrap_or("?").to_owned();
    }
    if let Some(use_bound) = bound.get("use") {
        let names: Vec<&str> = use_bound
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return format!("use<{}>", names.join(", "));
    }
    "?".to_owned()
}

/// Renders the traits of a trait object. The first entry is the principal
/// trait; the remaining auto traits are sorted because rustdoc emits them in
/// an order that differs between targets (`dyn Error + Sync + Send` on
/// aarch64-apple-darwin, `dyn Error + Send + Sync` on x86_64-unknown-linux-gnu).
fn render_poly_traits(traits: &Value) -> String {
    let mut rendered: Vec<String> = traits
        .as_array()
        .map(|traits| {
            traits
                .iter()
                .map(|poly| {
                    format!(
                        "{}{}",
                        render_hrtb(&poly["generic_params"]),
                        render_path(&poly["trait"])
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some((_, auto_traits)) = rendered.split_first_mut() {
        auto_traits.sort();
    }
    rendered.join(" + ")
}

fn render_hrtb(params: &Value) -> String {
    let names: Vec<String> = params
        .as_array()
        .map(|params| params.iter().map(render_generic_param).collect())
        .unwrap_or_default();
    if names.is_empty() {
        String::new()
    } else {
        format!("for<{}> ", names.join(", "))
    }
}

/// Renders one `GenericParamDef` (`T: Bound`, `'a`, `const N: usize`).
pub(super) fn render_generic_param(param: &Value) -> String {
    let name = param["name"].as_str().unwrap_or("?");
    let kind = &param["kind"];
    if let Some(lifetime) = kind.get("lifetime") {
        let outlives: Vec<&str> = lifetime["outlives"]
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return if outlives.is_empty() {
            name.to_owned()
        } else {
            format!("{name}: {}", outlives.join(" + "))
        };
    }
    if let Some(ty) = kind.get("type") {
        let bounds = render_bounds(&ty["bounds"]);
        let mut out = name.to_owned();
        if !bounds.is_empty() {
            out.push_str(": ");
            out.push_str(&bounds);
        }
        if let Some(default) = ty.get("default").filter(|default| !default.is_null()) {
            out.push_str(" = ");
            out.push_str(&render_type(default));
        }
        return out;
    }
    if let Some(constant) = kind.get("const") {
        let mut out = format!("const {name}: {}", render_type(&constant["type"]));
        if let Some(default) = constant["default"].as_str() {
            out.push_str(" = ");
            out.push_str(default);
        }
        return out;
    }
    name.to_owned()
}

/// Renders `Generics` as `<params>` plus ` where ...`, skipping synthetic
/// `impl Trait` parameters (already visible in the argument types).
pub(super) fn render_generics(generics: &Value) -> (String, String) {
    let params: Vec<String> = generics["params"]
        .as_array()
        .map(|params| {
            params
                .iter()
                .filter(|param| {
                    !param["kind"]["type"]["is_synthetic"]
                        .as_bool()
                        .unwrap_or(false)
                })
                .map(render_generic_param)
                .collect()
        })
        .unwrap_or_default();
    let predicates: Vec<String> = generics["where_predicates"]
        .as_array()
        .map(|predicates| predicates.iter().map(render_where_predicate).collect())
        .unwrap_or_default();
    let params = if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    };
    let predicates = if predicates.is_empty() {
        String::new()
    } else {
        format!(" where {}", predicates.join(", "))
    };
    (params, predicates)
}

fn render_where_predicate(predicate: &Value) -> String {
    if let Some(bound) = predicate.get("bound_predicate") {
        return format!(
            "{}{}: {}",
            render_hrtb(&bound["generic_params"]),
            render_type(&bound["type"]),
            render_bounds(&bound["bounds"])
        );
    }
    if let Some(lifetime) = predicate.get("lifetime_predicate") {
        let outlives: Vec<&str> = lifetime["outlives"]
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return format!(
            "{}: {}",
            lifetime["lifetime"].as_str().unwrap_or("?"),
            outlives.join(" + ")
        );
    }
    if let Some(eq) = predicate.get("eq_predicate") {
        let rhs = eq["rhs"]
            .get("type")
            .map_or_else(|| "?".to_owned(), render_type);
        return format!("{} = {rhs}", render_type(&eq["lhs"]));
    }
    "?".to_owned()
}
