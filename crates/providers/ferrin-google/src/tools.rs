//! Factories for Google provider-executed tools.
//!
//! Each factory returns a [`ferrin_tool::Tool`] whose definition is a
//! `ToolDefinition::Provider` with the `google.<tool>` id; the request
//! preparation converts the arguments to the wire format.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use serde::Serialize;
use serde_json::json;

use crate::prepare_tools::ids;

fn args<T: Serialize>(value: &T) -> JsonObject {
    match serde_json::to_value(value) {
        Ok(JsonValue::Object(mut object)) => {
            object.retain(|_, value| !value.is_null());
            object
        }
        _ => JsonObject::new(),
    }
}

fn executed(id: &str, args: JsonObject) -> Tool {
    Tool::provider_executed(id, args)
        .input_schema(Schema::any())
        .build()
}

/// Search types of `google.google_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTypes {
    /// Enable web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<JsonObject>,
    /// Enable image search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_search: Option<JsonObject>,
}

/// Time range filter of `google.google_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeRangeFilter {
    /// Start time (RFC 3339).
    pub start_time: String,
    /// End time (RFC 3339).
    pub end_time: String,
}

/// Arguments of `google.google_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSearchArgs {
    /// Search types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_types: Option<SearchTypes>,
    /// Time range filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range_filter: Option<TimeRangeFilter>,
}

/// Arguments of `google.file_search`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchArgs {
    /// File search store names (`fileSearchStores/...`).
    pub file_search_store_names: Vec<String>,
    /// Number of chunks to retrieve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Metadata filter expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_filter: Option<String>,
}

/// Arguments of `google.vertex_rag_store`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexRagStoreArgs {
    /// RAG corpus resource name.
    pub rag_corpus: String,
    /// Number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

/// Provider-executed tool factories.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleTools;

impl GoogleTools {
    /// Creates the factory namespace.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// `google.google_search`: Google Search grounding.
    #[must_use]
    pub fn google_search(&self, config: GoogleSearchArgs) -> Tool {
        executed(ids::GOOGLE_SEARCH, args(&config))
    }

    /// `google.enterprise_web_search` (Vertex AI).
    #[must_use]
    pub fn enterprise_web_search(&self) -> Tool {
        executed(ids::ENTERPRISE_WEB_SEARCH, JsonObject::new())
    }

    /// `google.url_context`: fetch URLs mentioned in the prompt.
    #[must_use]
    pub fn url_context(&self) -> Tool {
        executed(ids::URL_CONTEXT, JsonObject::new())
    }

    /// `google.code_execution`: server-side Python execution.
    #[must_use]
    pub fn code_execution(&self) -> Tool {
        Tool::provider_executed(ids::CODE_EXECUTION, JsonObject::new())
            .input_schema(Schema::from_json_schema(json!({
                "type": "object",
                "properties": {
                    "language": {"type": "string", "description": "The programming language of the code."},
                    "code": {"type": "string", "description": "The code to be executed."}
                },
                "required": ["language", "code"]
            })))
            .output_schema(Schema::from_json_schema(json!({
                "type": "object",
                "properties": {
                    "outcome": {"type": "string", "description": "The outcome of the execution (e.g., \"OUTCOME_OK\")."},
                    "output": {"type": "string", "description": "The output from the code execution."}
                },
                "required": ["outcome", "output"]
            })))
            .build()
    }

    /// `google.file_search`: retrieval from file search stores.
    #[must_use]
    pub fn file_search(&self, config: FileSearchArgs) -> Tool {
        executed(ids::FILE_SEARCH, args(&config))
    }

    /// `google.vertex_rag_store` (Vertex AI).
    #[must_use]
    pub fn vertex_rag_store(&self, config: VertexRagStoreArgs) -> Tool {
        executed(ids::VERTEX_RAG_STORE, args(&config))
    }

    /// `google.google_maps`: Google Maps grounding.
    #[must_use]
    pub fn google_maps(&self) -> Tool {
        executed(ids::GOOGLE_MAPS, JsonObject::new())
    }
}
