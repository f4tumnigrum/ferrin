//! Tool configuration parity with Vercel AI SDK `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; translated and modified.

use std::collections::HashMap;

use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderReference;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use serde_json::json;

use super::convert_tools::ConvertedTools;
use super::convert_tools::PROVIDER_TOOL_TYPES;
use super::options::AllowedToolsOptions;
use super::options::FunctionToolOptions;

#[derive(Clone, PartialEq)]
enum Resolution {
    Supported(JsonValue),
    Unsupported(String),
    Ambiguous,
}

pub(super) fn apply(
    converted: &mut ConvertedTools,
    definitions: &[ToolDefinition],
    allowed: Option<&AllowedToolsOptions>,
    mapping: &ToolNameMapping,
    supports_async: bool,
    provider_key: &str,
) -> Result<(), ProviderError> {
    if definitions.is_empty() {
        return Ok(());
    }
    if !supports_async && let Some(tools) = &mut converted.tools {
        for tool in tools {
            remove_async(tool, &mut converted.warnings);
        }
    }
    let Some(allowed) = allowed else {
        return Ok(());
    };
    if allowed.tools.is_empty()
        || allowed
            .mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "auto" | "required"))
    {
        return Err(InvalidArgumentError::new(
            "allowedTools",
            "allowedTools requires non-empty toolNames and an auto or required mode",
        )
        .into());
    }
    let mut direct = HashMap::new();
    let mut aliases = HashMap::new();
    for tool in definitions {
        let name = tool.name().as_str();
        let resolution = match tool {
            ToolDefinition::Function {
                provider_options, ..
            } => {
                let options = provider_options
                    .as_ref()
                    .map(|options| {
                        parse_provider_options::<FunctionToolOptions>(provider_key, options)
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or_default();
                if options.namespace.is_some() {
                    Resolution::Unsupported("tools inside an OpenAI tool namespace are not visible to tool_choice.allowed_tools".into())
                } else if options.defer_loading == Some(true) {
                    Resolution::Unsupported(
                        "deferred tools are not visible to tool_choice.allowed_tools".into(),
                    )
                } else {
                    Resolution::Supported(json!({"type":"function","name":name}))
                }
            }
            ToolDefinition::Provider { id, args, .. } => {
                let Some((_, kind)) = PROVIDER_TOOL_TYPES.iter().find(|(known, _)| *known == id)
                else {
                    continue;
                };
                match *kind {
                    "custom" => Resolution::Supported(json!({"type":"custom","name":name})),
                    "mcp" => Resolution::Supported(
                        json!({"type":"mcp","server_label":args.get("serverLabel")}),
                    ),
                    "tool_search" => Resolution::Unsupported(
                        "OpenAI does not support tool_search tools in tool_choice.allowed_tools"
                            .into(),
                    ),
                    _ => Resolution::Supported(json!({"type":kind})),
                }
            }
            _ => continue,
        };
        direct.insert(name.to_owned(), resolution.clone());
        let canonical = mapping.to_provider_tool_name(name);
        if canonical != name {
            aliases
                .entry(canonical.to_owned())
                .and_modify(|current| {
                    if *current != resolution {
                        *current = Resolution::Ambiguous;
                    }
                })
                .or_insert(resolution);
        }
    }
    let mut entries = Vec::new();
    for name in &allowed.tools {
        let explicit = direct.get(name);
        let resolution = explicit.or_else(|| aliases.get(name));
        if explicit.is_some() && aliases.contains_key(name) {
            warn(
                converted,
                name,
                "this name is both a tool name and the provider tool name of another tool in this request; the tool with this name is allowed",
            );
        }
        match resolution {
            Some(Resolution::Supported(entry)) => entries.push(entry.clone()),
            Some(Resolution::Unsupported(reason)) => warn(
                converted,
                name,
                &format!("{reason}; the tool is removed from the allowed tools"),
            ),
            Some(Resolution::Ambiguous) => warn(
                converted,
                name,
                "several tools in this request share this provider tool name; use the tool name from the tools for this request instead",
            ),
            None => {
                warn(
                    converted,
                    name,
                    "the tool is not part of the tools for this request and is sent as a function tool",
                );
                entries.push(json!({"type":"function","name":mapping.to_provider_tool_name(name)}));
            }
        }
    }
    if entries.is_empty() {
        return Err(ProviderError::unsupported(
            "allowedTools with only tools that cannot be allow-listed",
        ));
    }
    converted.tool_choice = Some(
        json!({"type":"allowed_tools","mode":allowed.mode.as_deref().unwrap_or("auto"),"tools":entries}),
    );
    Ok(())
}

fn warn(converted: &mut ConvertedTools, name: &str, details: &str) {
    converted.warnings.push(Warning::unsupported_with_details(
        format!("allowedTools entry \"{name}\""),
        details,
    ));
}

fn remove_async(tool: &mut JsonValue, warnings: &mut Vec<Warning>) {
    if tool.get("async") == Some(&JsonValue::Bool(true)) {
        let name = tool
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        warnings.push(Warning::unsupported_with_details(
            format!("async tool calling for \"{name}\""),
            "Async tool calling is only supported by GPT-6 and later models.",
        ));
        if let Some(object) = tool.as_object_mut() {
            object.remove("async");
        }
    }
    if let Some(children) = tool.get_mut("tools").and_then(JsonValue::as_array_mut) {
        for child in children {
            remove_async(child, warnings);
        }
    }
}

pub(super) fn shell_environment(environment: &JsonValue) -> Result<JsonValue, ProviderError> {
    let kind = environment
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or("local");
    let wire_type = match kind {
        "containerAuto" => "container_auto",
        "containerReference" => "container_reference",
        _ => "local",
    };
    let mut result = JsonObject::from_iter([("type".into(), json!(wire_type))]);
    for (key, wire) in [
        ("containerId", "container_id"),
        ("fileIds", "file_ids"),
        ("memoryLimit", "memory_limit"),
    ] {
        if let Some(value) = environment.get(key) {
            result.insert(wire.into(), value.clone());
        }
    }
    if let Some(network) = environment.get("networkPolicy") {
        let mut policy = JsonObject::new();
        for (key, wire) in [
            ("type", "type"),
            ("allowedDomains", "allowed_domains"),
            ("domainSecrets", "domain_secrets"),
        ] {
            if let Some(value) = network.get(key) {
                policy.insert(wire.into(), value.clone());
            }
        }
        result.insert("network_policy".into(), JsonValue::Object(policy));
    }
    if let Some(skills) = environment.get("skills").and_then(JsonValue::as_array) {
        let mut mapped = Vec::new();
        for skill in skills {
            mapped.push(if wire_type=="local" { skill.clone() }
            else if skill.get("type").and_then(JsonValue::as_str)==Some("skillReference") {
                let reference:ProviderReference=serde_json::from_value(skill.get("providerReference").cloned().unwrap_or_else(||json!({}))).map_err(ProviderError::other)?;
                json!({"type":"skill_reference","skill_id":resolve_provider_reference(&reference,"openai")?,"version":skill.get("version").cloned().unwrap_or_else(||json!("latest"))})
            } else {
                json!({"type":"inline","name":skill.get("name"),"description":skill.get("description"),"source":{"type":"base64","media_type":skill["source"].get("mediaType"),"data":skill["source"].get("data")}})
            });
        }
        result.insert("skills".into(), JsonValue::Array(mapped));
    }
    Ok(JsonValue::Object(result))
}
