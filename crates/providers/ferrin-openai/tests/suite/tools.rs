//! Provider tool factories and their wire conversion.

use ferrin_openai::responses::convert_tools::convert_tools;
use ferrin_openai::responses::convert_tools::tool_name_mapping;
use ferrin_openai::tools::CodeInterpreterArgs;
use ferrin_openai::tools::CodeInterpreterContainer;
use ferrin_openai::tools::CustomToolArgs;
use ferrin_openai::tools::CustomToolFormat;
use ferrin_openai::tools::FileSearchArgs;
use ferrin_openai::tools::FileSearchRanking;
use ferrin_openai::tools::McpArgs;
use ferrin_openai::tools::OpenAiTools;
use ferrin_openai::tools::UserLocation;
use ferrin_openai::tools::WebSearchArgs;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_tool::Tool;
use ferrin_tool::ToolKind;
use pretty_assertions::assert_eq;
use serde_json::json;

fn definition(tool: &Tool, name: &str) -> ToolDefinition {
    tool.definition(name.into(), None)
}

#[test]
fn factories_produce_provider_tool_definitions() {
    let tools = OpenAiTools::new();
    let web = tools.web_search(WebSearchArgs {
        search_context_size: Some("low".to_owned()),
        user_location: Some(UserLocation {
            city: Some("Paris".to_owned()),
            ..UserLocation::approximate()
        }),
        ..WebSearchArgs::default()
    });
    assert!(
        matches!(web.kind(), ToolKind::ProviderExecuted { id, .. } if id == "openai.web_search")
    );
    let ToolDefinition::Provider { id, name, args } = definition(&web, "web_search") else {
        panic!("expected provider tool");
    };
    assert_eq!(id, "openai.web_search");
    assert_eq!(name.as_str(), "web_search");
    assert_eq!(args["searchContextSize"], json!("low"));
    assert_eq!(args["userLocation"]["city"], json!("Paris"));

    let shell = tools.local_shell();
    assert!(
        matches!(shell.kind(), ToolKind::ProviderDefined { id, .. } if id == "openai.local_shell")
    );
}

#[test]
fn provider_tools_convert_to_snake_case_wire_items() {
    let tools = OpenAiTools::new();
    let definitions = vec![
        definition(
            &tools.web_search(WebSearchArgs {
                search_context_size: Some("medium".to_owned()),
                user_location: Some(UserLocation {
                    country: Some("FR".to_owned()),
                    ..UserLocation::approximate()
                }),
                ..WebSearchArgs::default()
            }),
            "web_search",
        ),
        definition(
            &tools.file_search(FileSearchArgs {
                vector_store_ids: vec!["vs_1".to_owned()],
                max_num_results: Some(5),
                ranking: Some(FileSearchRanking {
                    ranker: Some("auto".to_owned()),
                    score_threshold: Some(0.5),
                }),
                filters: None,
            }),
            "file_search",
        ),
        definition(
            &tools.code_interpreter(CodeInterpreterArgs {
                container: Some(CodeInterpreterContainer::Auto {
                    file_ids: Some(vec!["file-1".to_owned()]),
                }),
            }),
            "code_interpreter",
        ),
        definition(
            &tools.mcp(McpArgs {
                server_label: "docs".to_owned(),
                server_url: Some("https://mcp.example.test".to_owned()),
                require_approval: Some(json!("never")),
                ..McpArgs::default()
            }),
            "mcp",
        ),
        definition(
            &tools.custom(CustomToolArgs {
                name: "sql".to_owned(),
                description: Some("Run SQL".to_owned()),
                format: Some(CustomToolFormat::Grammar {
                    syntax: "lark".to_owned(),
                    definition: "start: \"SELECT\"".to_owned(),
                }),
            }),
            "sql",
        ),
        ToolDefinition::function(
            "get_weather",
            Some("Weather".to_owned()),
            json!({"type": "object", "properties": {"city": {"type": "string"}}, "propertyNames": {"type": "string"}}),
        ),
    ];
    let mapping = tool_name_mapping(&definitions);
    let converted = convert_tools(
        &definitions,
        Some(&ToolChoice::Tool {
            tool_name: "web_search".into(),
        }),
        &mapping,
        true,
        "openai",
    )
    .unwrap();
    assert!(converted.has_web_search);
    assert!(converted.has_code_interpreter);
    assert!(converted.provider_tools.custom_tool_names.contains("sql"));
    assert_eq!(converted.tool_choice, Some(json!({"type": "web_search"})));
    assert_eq!(converted.warnings.len(), 1, "{:?}", converted.warnings);
    insta::assert_json_snapshot!("tools_wire_items", converted.tools.unwrap());
}

#[test]
fn unknown_provider_tools_produce_a_warning() {
    let definitions = vec![ToolDefinition::Provider {
        id: "other.magic".to_owned(),
        name: "magic".into(),
        args: ferrin_spec::JsonObject::new(),
    }];
    let mapping = tool_name_mapping(&definitions);
    let converted = convert_tools(&definitions, None, &mapping, true, "openai").unwrap();
    assert!(converted.tools.is_none());
    assert_eq!(converted.warnings.len(), 1);
}

#[test]
fn provider_tool_options_preserve_opaque_dictionary_keys() {
    let definitions = vec![ToolDefinition::Provider {
        id: "openai.mcp".to_owned(),
        name: "remote".into(),
        args: serde_json::from_value(json!({
            "serverLabel": "docs", "serverUrl": "https://example.test/mcp",
            "headers": {"X-API-Key": "test-key", "serverUrl": "opaque"},
            "metadata": {"readOnly": true},
            "allowedTools": {"readOnly": true, "toolNames": ["getData"]},
            "requireApproval": {"never": {"toolNames": ["getData"]}},
            "parameters": {"type": "object", "properties": {"serverUrl": {"type": "string"}}}
        }))
        .unwrap(),
    }];
    let mapping = tool_name_mapping(&definitions);
    let converted = convert_tools(&definitions, None, &mapping, false, "openai").unwrap();
    assert_eq!(
        converted.tools,
        Some(vec![json!({
            "type": "mcp", "server_label": "docs", "server_url": "https://example.test/mcp",
            "headers": {"X-API-Key": "test-key", "serverUrl": "opaque"},
            "metadata": {"readOnly": true},
            "allowed_tools": {"read_only": true, "tool_names": ["getData"]},
            "require_approval": {"never": {"tool_names": ["getData"]}},
            "parameters": {"type": "object", "properties": {"serverUrl": {"type": "string"}}}
        })])
    );
}
