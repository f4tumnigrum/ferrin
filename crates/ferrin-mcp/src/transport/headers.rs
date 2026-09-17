//! `x-mcp-header` bindings: tool parameters mirrored into `Mcp-Param-*`
//! request headers (protocol 2026-07-28).
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;

use crate::error::McpError;

/// JSON type of a bound parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeaderValueType {
    /// `boolean`.
    Boolean,
    /// `integer`.
    Integer,
    /// `string`.
    String,
}

/// One parameter bound to a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderBinding {
    /// Header name suffix (`Mcp-Param-<name>`).
    pub header_name: String,
    /// Property path inside the arguments object.
    pub path: Vec<String>,
    /// Expected value type.
    pub value_type: HeaderValueType,
}

fn is_http_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Encodes a header value: plain printable ASCII without surrounding
/// whitespace is sent as-is, everything else as `=?base64?<data>?=`.
#[must_use]
pub fn encode_header_value(value: &str) -> String {
    let plain_ascii = value
        .chars()
        .all(|character| character == '\t' || ('\u{20}'..='\u{7e}').contains(&character));
    let looks_encoded = value.starts_with("=?base64?") && value.ends_with("?=");
    if plain_ascii && value.trim() == value && !looks_encoded {
        return value.to_owned();
    }
    format!("=?base64?{}?=", STANDARD.encode(value.as_bytes()))
}

struct Visitor {
    bindings: Vec<HeaderBinding>,
    names: Vec<String>,
    error: Option<String>,
}

impl Visitor {
    fn visit(&mut self, value: &JsonValue, path: &[String], statically_reachable: bool) {
        if self.error.is_some() {
            return;
        }
        let Some(object) = value.as_object() else {
            return;
        };
        if let Some(header) = object.get("x-mcp-header") {
            if !statically_reachable || path.is_empty() {
                self.error =
                    Some("x-mcp-header is not on a statically reachable property".to_owned());
                return;
            }
            let Some(name) = header.as_str().filter(|name| is_http_token(name)) else {
                self.error = Some("x-mcp-header must be a non-empty HTTP token".to_owned());
                return;
            };
            let lower = name.to_ascii_lowercase();
            if self.names.contains(&lower) {
                self.error = Some(format!("x-mcp-header value \"{name}\" is not unique"));
                return;
            }
            let value_type = match object.get("type").and_then(JsonValue::as_str) {
                Some("boolean") => HeaderValueType::Boolean,
                Some("integer") => HeaderValueType::Integer,
                Some("string") => HeaderValueType::String,
                _ => {
                    self.error = Some(
                        "x-mcp-header can only annotate boolean, integer, or string properties"
                            .to_owned(),
                    );
                    return;
                }
            };
            self.names.push(lower);
            self.bindings.push(HeaderBinding {
                header_name: name.to_owned(),
                path: path.to_vec(),
                value_type,
            });
        }
        for (key, child) in object {
            if key == "x-mcp-header" {
                continue;
            }
            if key == "properties"
                && let Some(properties) = child.as_object()
            {
                for (property, schema) in properties {
                    let mut child_path = path.to_vec();
                    child_path.push(property.clone());
                    self.visit(schema, &child_path, statically_reachable);
                }
            } else {
                self.visit(child, path, false);
            }
        }
    }
}

/// Extracts the header bindings of a tool input schema.
///
/// # Errors
///
/// Returns [`McpError::InvalidArgument`] when a binding sits on a property
/// that is not statically reachable, has an invalid header name, repeats a
/// header name or annotates a non-scalar property.
pub fn header_bindings(input_schema: &JsonValue) -> Result<Vec<HeaderBinding>, McpError> {
    if !input_schema.is_object() {
        return Err(McpError::invalid_argument(
            "inputSchema must be a JSON Schema object",
        ));
    }
    let mut visitor = Visitor {
        bindings: Vec::new(),
        names: Vec::new(),
        error: None,
    };
    visitor.visit(input_schema, &[], true);
    match visitor.error {
        Some(error) => Err(McpError::invalid_argument(error)),
        None => Ok(visitor.bindings),
    }
}

fn value_at<'a>(arguments: &'a JsonObject, path: &[String]) -> Option<&'a JsonValue> {
    let (first, rest) = path.split_first()?;
    let mut current = arguments.get(first)?;
    for segment in rest {
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

/// Builds the `Mcp-Param-*` headers of a `tools/call` from its arguments.
///
/// # Errors
///
/// Returns [`McpError::InvalidArgument`] when an argument does not match its
/// declared header type.
pub fn tool_headers(
    bindings: &[HeaderBinding],
    arguments: &JsonObject,
) -> Result<Vec<(String, String)>, McpError> {
    let mut headers = Vec::new();
    for binding in bindings {
        let Some(value) = value_at(arguments, &binding.path).filter(|value| !value.is_null())
        else {
            continue;
        };
        let text = match (binding.value_type, value) {
            (HeaderValueType::String, JsonValue::String(text)) => text.clone(),
            (HeaderValueType::Boolean, JsonValue::Bool(flag)) => flag.to_string(),
            (HeaderValueType::Integer, JsonValue::Number(number)) => match number.as_f64() {
                Some(value) if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 => {
                    if value == 0.0 {
                        "0".to_owned()
                    } else {
                        format!("{value:.0}")
                    }
                }
                _ => return Err(invalid_header_type(binding)),
            },
            _ => {
                return Err(invalid_header_type(binding));
            }
        };
        headers.push((
            format!("Mcp-Param-{}", binding.header_name),
            encode_header_value(&text),
        ));
    }
    Ok(headers)
}

fn invalid_header_type(binding: &HeaderBinding) -> McpError {
    McpError::invalid_argument(format!(
        "tool argument \"{}\" does not match its x-mcp-header type",
        binding.path.join(".")
    ))
}
