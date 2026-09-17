//! Interactions tool conversion, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn provider_tool(kind: &str, args: &JsonObject) -> Option<JsonValue> {
    let fields: &[(&str, &str)] = match kind {
        "google_search" | "code_execution" | "url_context" => &[],
        "file_search" => &[
            ("fileSearchStoreNames", "file_search_store_names"),
            ("topK", "top_k"),
            ("metadataFilter", "metadata_filter"),
        ],
        "google_maps" => &[
            ("latitude", "latitude"),
            ("longitude", "longitude"),
            ("enableWidget", "enable_widget"),
        ],
        "computer_use" => &[
            ("environment", "environment"),
            ("excludedPredefinedFunctions", "excludedPredefinedFunctions"),
        ],
        "mcp_server" => &[
            ("name", "name"),
            ("url", "url"),
            ("headers", "headers"),
            ("allowedTools", "allowed_tools"),
        ],
        "retrieval" => &[
            ("retrievalTypes", "retrieval_types"),
            ("vertexAiSearchConfig", "vertex_ai_search_config"),
        ],
        _ => return None,
    };
    let mut tool = json!({"type":kind});
    for (key, wire) in fields {
        if let Some(value) = args.get(*key).filter(|value| !value.is_null()) {
            tool[wire] = value.clone();
        }
    }
    match kind {
        "google_search" => {
            if let Some(search) = args.get("searchTypes").and_then(JsonValue::as_object) {
                let types: Vec<_> = [("webSearch", "web_search"), ("imageSearch", "image_search")]
                    .into_iter()
                    .filter_map(|(key, wire)| {
                        search
                            .get(key)
                            .filter(|value| !value.is_null())
                            .map(|_| wire)
                    })
                    .collect();
                if !types.is_empty() {
                    tool["search_types"] = json!(types);
                }
            }
        }
        "computer_use" if tool.get("environment").is_none() => {
            tool["environment"] = json!("browser");
        }
        "retrieval" if tool.get("retrieval_types").is_none() => {
            tool["retrieval_types"] = json!(["vertex_ai_search"]);
        }
        _ => {}
    }
    Some(tool)
}
