//! Provider tool factories and their wire conversion.

use ferrin_anthropic::cache_control::CacheControlValidator;
use ferrin_anthropic::prepare_tools::PrepareToolsSettings;
use ferrin_anthropic::prepare_tools::prepare_tools;
use ferrin_anthropic::tools::AdvisorArgs;
use ferrin_anthropic::tools::AdvisorCaching;
use ferrin_anthropic::tools::AnthropicTools;
use ferrin_anthropic::tools::CitationsArg;
use ferrin_anthropic::tools::ComputerArgs;
use ferrin_anthropic::tools::TextEditorArgs;
use ferrin_anthropic::tools::UserLocation;
use ferrin_anthropic::tools::WebFetchArgs;
use ferrin_anthropic::tools::WebSearchArgs;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_tool::Tool;
use ferrin_tool::ToolKind;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::anthropic_options;

fn definition(tool: &Tool, name: &str) -> ToolDefinition {
    tool.definition(name.into(), None)
}

fn settings() -> PrepareToolsSettings {
    PrepareToolsSettings {
        disable_parallel_tool_use: false,
        supports_structured_output: true,
        supports_strict_tools: true,
        default_eager_input_streaming: false,
    }
}

fn weather_tool() -> ToolDefinition {
    ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )
}

#[test]
fn factories_produce_provider_tool_definitions() {
    let tools = AnthropicTools::new();
    let web = tools.web_search_20260209(WebSearchArgs {
        max_uses: Some(2),
        user_location: Some(UserLocation {
            city: Some("Paris".to_owned()),
            ..UserLocation::approximate()
        }),
        ..WebSearchArgs::default()
    });
    assert!(matches!(
        web.kind(),
        ToolKind::ProviderExecuted { id, .. } if id == "anthropic.web_search_20260209"
    ));
    let ToolDefinition::Provider { id, name, args } = definition(&web, "web_search") else {
        panic!("expected provider tool");
    };
    assert_eq!(id, "anthropic.web_search_20260209");
    assert_eq!(name.as_str(), "web_search");
    assert_eq!(args["maxUses"], json!(2));
    assert_eq!(args["userLocation"]["city"], json!("Paris"));
    assert_eq!(args["userLocation"]["type"], json!("approximate"));
    assert!(args.get("allowedDomains").is_none());

    let bash = tools.bash_20250124();
    assert!(matches!(
        bash.kind(),
        ToolKind::ProviderDefined { id, .. } if id == "anthropic.bash_20250124"
    ));
}

#[tokio::test]
async fn prepare_tools_wire_snapshot() {
    let test = TestProvider::start().await;
    let factories = AnthropicTools::new();
    let mut function = weather_tool();
    if let ToolDefinition::Function {
        strict,
        input_examples,
        provider_options,
        ..
    } = &mut function
    {
        *strict = Some(true);
        input_examples.push(json!({"city": "Paris"}).as_object().cloned().unwrap());
        *provider_options = Some(anthropic_options(json!({
            "cacheControl": {"type": "ephemeral"},
            "deferLoading": true,
            "allowedCallers": ["code_execution_20250825"],
            "eagerInputStreaming": true
        })));
    }
    let tools = vec![
        function,
        definition(&factories.bash_20250124(), "bash"),
        definition(
            &factories.computer_20250124(ComputerArgs {
                display_width_px: 1280,
                display_height_px: 800,
                display_number: Some(1),
                enable_zoom: None,
            }),
            "computer",
        ),
        definition(
            &factories.text_editor_20250728(TextEditorArgs {
                max_characters: Some(10_000),
            }),
            "str_replace_based_edit_tool",
        ),
        definition(&factories.memory_20250818(), "memory"),
        definition(
            &factories.web_search_20250305(WebSearchArgs {
                max_uses: Some(3),
                allowed_domains: Some(vec!["example.test".to_owned()]),
                ..WebSearchArgs::default()
            }),
            "web_search",
        ),
        definition(
            &factories.web_fetch_20250910(WebFetchArgs {
                citations: Some(CitationsArg { enabled: true }),
                max_content_tokens: Some(5000),
                ..WebFetchArgs::default()
            }),
            "web_fetch",
        ),
        definition(&factories.code_execution_20250825(), "code_execution"),
        definition(
            &factories.tool_search_bm25_20251119(),
            "tool_search_tool_bm25",
        ),
        definition(
            &factories.advisor_20260301(AdvisorArgs {
                model: "claude-opus-4-6".to_owned(),
                max_uses: Some(1),
                max_tokens: Some(2048),
                caching: Some(AdvisorCaching::ephemeral("5m")),
            }),
            "advisor",
        ),
    ];
    let mut cache = CacheControlValidator::new();
    let prepared = prepare_tools(
        test.provider.config(),
        &tools,
        Some(&ToolChoice::Auto),
        settings(),
        &mut cache,
    );
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert!(cache.into_warnings().is_empty());
    assert_eq!(prepared.tool_choice, Some(json!({"type": "auto"})));
    let betas: Vec<&String> = prepared.betas.iter().collect();
    assert_eq!(
        betas,
        vec![
            "advanced-tool-use-2025-11-20",
            "advisor-tool-2026-03-01",
            "code-execution-2025-08-25",
            "computer-use-2025-01-24",
            "context-management-2025-06-27",
            "structured-outputs-2025-11-13",
            "web-fetch-2025-09-10",
        ]
    );
    insta::assert_json_snapshot!("tools_wire_items", prepared.tools.unwrap());
}

#[tokio::test]
async fn tool_choice_is_mapped_and_none_drops_the_tools() {
    let test = TestProvider::start().await;
    let tools = vec![weather_tool()];
    let mut cache = CacheControlValidator::new();
    let prepared = prepare_tools(test.provider.config(), &tools, None, settings(), &mut cache);
    assert_eq!(prepared.tools.as_ref().map(Vec::len), Some(1));
    assert_eq!(prepared.tool_choice, None);

    let prepared = prepare_tools(
        test.provider.config(),
        &tools,
        Some(&ToolChoice::None),
        settings(),
        &mut cache,
    );
    assert_eq!(prepared.tools, None);
    assert_eq!(prepared.tool_choice, None);

    let mut parallel_off = settings();
    parallel_off.disable_parallel_tool_use = true;
    let prepared = prepare_tools(
        test.provider.config(),
        &tools,
        Some(&ToolChoice::Required),
        parallel_off,
        &mut cache,
    );
    assert_eq!(
        prepared.tool_choice,
        Some(json!({"type": "any", "disable_parallel_tool_use": true}))
    );

    let prepared = prepare_tools(
        test.provider.config(),
        &tools,
        Some(&ToolChoice::Tool {
            tool_name: "get_weather".into(),
        }),
        settings(),
        &mut cache,
    );
    assert_eq!(
        prepared.tool_choice,
        Some(json!({"type": "tool", "name": "get_weather"}))
    );
}

#[tokio::test]
async fn unsupported_tools_and_strict_produce_warnings() {
    let test = TestProvider::start().await;
    let mut strict = weather_tool();
    if let ToolDefinition::Function { strict, .. } = &mut strict {
        *strict = Some(true);
    }
    let tools = vec![
        strict,
        ToolDefinition::provider("anthropic.mystery_20990101", "mystery", Default::default()),
    ];
    let mut no_strict = settings();
    no_strict.supports_strict_tools = false;
    let mut cache = CacheControlValidator::new();
    let prepared = prepare_tools(test.provider.config(), &tools, None, no_strict, &mut cache);
    let converted = prepared.tools.unwrap();
    assert_eq!(converted.len(), 1);
    assert!(converted[0].get("strict").is_none());
    assert_eq!(prepared.warnings.len(), 2, "{:?}", prepared.warnings);
    assert!(matches!(
        &prepared.warnings[0],
        Warning::Unsupported { feature, .. } if feature == "strict"
    ));
    assert!(matches!(
        &prepared.warnings[1],
        Warning::Unsupported { feature, .. } if feature == "tool: mystery"
    ));
}

#[tokio::test]
async fn provider_tool_aliases_roundtrip_through_choices_and_calls() {
    use ferrin_anthropic::output::OutputMapper;
    use ferrin_anthropic::prepare_tools::tool_name_mapping;
    use ferrin_anthropic::stream::AnthropicStreamState;
    use ferrin_provider_util::http::ParseResult;
    use ferrin_provider_util::stream_driver::StreamMachine;
    use ferrin_spec::Content;
    use ferrin_spec::StreamPart;
    use ferrin_spec::ToolCall;

    let test = TestProvider::start().await;
    let tools = vec![definition(&AnthropicTools::new().bash_20250124(), "shell")];
    let prepared = prepare_tools(
        test.provider.config(),
        &tools,
        Some(&ToolChoice::Tool {
            tool_name: "shell".into(),
        }),
        settings(),
        &mut CacheControlValidator::new(),
    );
    assert_eq!(
        prepared.tool_choice,
        Some(json!({"type":"tool", "name":"bash"}))
    );
    assert_eq!(prepared.tools.unwrap()[0]["name"], json!("bash"));
    let wire = json!({"type":"tool_use", "id":"call-1", "name":"bash", "input":{"command":"pwd"}});
    let mut mapper = OutputMapper::new(test.provider.config().clone(), tool_name_mapping(&tools));
    assert_eq!(
        mapper.map_block(&serde_json::from_value(wire.clone()).unwrap()),
        vec![Content::ToolCall(ToolCall::new(
            "call-1",
            "shell",
            "{\"command\":\"pwd\"}"
        ))]
    );
    for chunks in [
        vec![json!({"type":"message_start", "message":{"id":"msg-1", "content":[wire]}})],
        vec![
            json!({"type":"content_block_start", "index":0, "content_block":wire}),
            json!({"type":"content_block_stop", "index":0}),
        ],
    ] {
        let mapper = OutputMapper::new(test.provider.config().clone(), tool_name_mapping(&tools));
        let mut state = AnthropicStreamState::new(mapper, None);
        let names: Vec<_> = chunks
            .into_iter()
            .flat_map(|raw| {
                state.handle(
                    ParseResult::Ok {
                        value: serde_json::from_value(raw.clone()).unwrap(),
                        raw,
                    },
                    false,
                )
            })
            .filter_map(|part| match part {
                StreamPart::ToolInputStart { tool_name, .. } => Some(tool_name),
                StreamPart::ToolCall(call) => Some(call.tool_name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["shell", "shell"]);
    }
}
