//! Provider-executed tool factories and their wire format.

use ferrin_google::capabilities::capabilities;
use ferrin_google::prepare_tools::PreparedTools;
use ferrin_google::prepare_tools::prepare_tools;
use ferrin_google::tools::FileSearchArgs;
use ferrin_google::tools::GoogleSearchArgs;
use ferrin_google::tools::GoogleTools;
use ferrin_google::tools::TimeRangeFilter;
use ferrin_google::tools::VertexRagStoreArgs;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

fn provider_tools() -> Vec<ToolDefinition> {
    let tools = GoogleTools::new();
    vec![
        tools
            .google_search(GoogleSearchArgs {
                search_types: None,
                time_range_filter: Some(TimeRangeFilter {
                    start_time: "2026-01-01T00:00:00Z".to_owned(),
                    end_time: "2026-09-01T00:00:00Z".to_owned(),
                }),
            })
            .definition("google_search".into(), None),
        tools
            .enterprise_web_search()
            .definition("enterprise_web_search".into(), None),
        tools.url_context().definition("url_context".into(), None),
        tools
            .code_execution()
            .definition("code_execution".into(), None),
        tools
            .file_search(FileSearchArgs {
                file_search_store_names: vec!["fileSearchStores/store-1".to_owned()],
                top_k: Some(5),
                metadata_filter: None,
            })
            .definition("file_search".into(), None),
        tools
            .vertex_rag_store(VertexRagStoreArgs {
                rag_corpus: "projects/p/locations/us/ragCorpora/c".to_owned(),
                top_k: Some(3),
            })
            .definition("vertex_rag_store".into(), None),
        tools.google_maps().definition("google_maps".into(), None),
    ]
}

fn function_tool() -> ToolDefinition {
    ToolDefinition::function(
        "get_weather",
        None,
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )
}

fn prepare(tools: &[ToolDefinition], choice: Option<&ToolChoice>, model: &str) -> PreparedTools {
    prepare_tools(
        tools,
        choice,
        capabilities(model),
        &ToolNameMapping::new(tools, &std::collections::HashMap::new()),
        None,
    )
    .unwrap()
}

#[test]
fn provider_tools_map_to_the_wire_format() {
    let prepared = prepare(&provider_tools(), None, "gemini-2.5-flash");
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("tools_provider", prepared.tools);
    assert!(prepared.tool_config.is_none());
}

#[test]
fn provider_tools_need_gemini_2_capabilities() {
    let prepared = prepare(&provider_tools()[..4], None, "gemini-1.5-pro");
    assert_eq!(prepared.tools, None);
    assert_eq!(prepared.warnings.len(), 4, "{:?}", prepared.warnings);
    let unknown = vec![ToolDefinition::provider(
        "openai.web_search",
        "web_search",
        ferrin_spec::JsonObject::new(),
    )];
    let prepared = prepare(&unknown, None, "gemini-2.5-flash");
    assert_eq!(prepared.tools, None);
    assert_eq!(prepared.warnings.len(), 1);
}

#[test]
fn mixed_tools_are_only_combined_on_gemini_3() {
    let mut tools = vec![function_tool()];
    tools.push(provider_tools().remove(0));
    let prepared = prepare(&tools, None, "gemini-2.5-flash");
    assert_eq!(prepared.warnings.len(), 1, "{:?}", prepared.warnings);
    assert_eq!(prepared.tools.as_ref().map(Vec::len), Some(1));
    assert!(prepared.tools.unwrap()[0].get("googleSearch").is_some());

    let prepared = prepare(
        &tools,
        Some(&ToolChoice::Tool {
            tool_name: "get_weather".into(),
        }),
        "gemini-3-pro-preview",
    );
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("tools_mixed_gemini3", prepared.tools);
    assert_eq!(
        prepared.tool_config,
        Some(
            json!({
                "functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["get_weather"]},
                "includeServerSideToolInvocations": true
            })
            .as_object()
            .unwrap()
            .clone()
        )
    );
}

#[test]
fn function_tool_choices_map_to_calling_modes() {
    let tools = vec![function_tool()];
    let prepared = prepare(&tools, Some(&ToolChoice::Auto), "gemini-2.5-flash");
    assert_eq!(
        prepared.tool_config.unwrap()["functionCallingConfig"]["mode"],
        json!("AUTO")
    );
    let prepared = prepare(&tools, Some(&ToolChoice::Required), "gemini-2.5-flash");
    assert_eq!(
        prepared.tool_config.unwrap()["functionCallingConfig"]["mode"],
        json!("ANY")
    );
    let prepared = prepare(&tools, None, "gemini-2.5-flash");
    assert!(prepared.tool_config.is_none());
    assert_eq!(
        prepared.tools.unwrap()[0]["functionDeclarations"][0]["description"],
        json!("")
    );
}
