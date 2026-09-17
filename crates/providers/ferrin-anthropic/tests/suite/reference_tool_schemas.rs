//! Every existing provider factory checked against reference Zod parsing fixtures.

use ferrin_anthropic::tools::AnthropicTools;
use ferrin_tool::Tool;
use pretty_assertions::assert_eq;
use serde_json::Value;

fn check(tool: Tool, fixture: &str) {
    let case: Value = serde_json::from_str(fixture).unwrap();
    let id = case["tool"].as_str().unwrap();
    let input = &case["input"];
    assert_eq!(
        tool.validate_input(&id.into(), input["value"].clone())
            .unwrap(),
        input["expected"],
        "{id} input"
    );
    for invalid in input["invalid"].as_array().unwrap() {
        assert!(
            tool.validate_input(&id.into(), invalid.clone()).is_err(),
            "{id} accepted invalid input"
        );
    }
    if let Some(output) = case.get("output") {
        let schema = tool
            .output_schema()
            .expect("reference supplies output schema");
        assert_eq!(
            schema.validate(output["value"].clone()).unwrap(),
            output["expected"],
            "{id} output"
        );
        for invalid in output["invalid"].as_array().unwrap() {
            assert!(
                schema.validate(invalid.clone()).is_err(),
                "{id} accepted invalid output"
            );
        }
    } else {
        assert!(
            tool.output_schema().is_none(),
            "{id} invents an output constraint"
        );
    }
}

#[test]
fn text_editor_20250728_matches_reference_parsing() {
    check(
        AnthropicTools::new().text_editor_20250728(Default::default()),
        include_str!("../fixtures/tool-schemas/text_editor_20250728.json"),
    );
}

#[test]
fn web_fetch_20250910_matches_reference_parsing() {
    check(
        AnthropicTools::new().web_fetch_20250910(Default::default()),
        include_str!("../fixtures/tool-schemas/web_fetch_20250910.json"),
    );
}

#[test]
fn tool_search_bm25_20251119_matches_reference_parsing() {
    check(
        AnthropicTools::new().tool_search_bm25_20251119(),
        include_str!("../fixtures/tool-schemas/tool_search_bm25_20251119.json"),
    );
}

#[test]
fn code_execution_20250522_matches_reference_parsing() {
    check(
        AnthropicTools::new().code_execution_20250522(),
        include_str!("../fixtures/tool-schemas/code_execution_20250522.json"),
    );
}

#[test]
fn web_fetch_20260209_matches_reference_parsing() {
    check(
        AnthropicTools::new().web_fetch_20260209(Default::default()),
        include_str!("../fixtures/tool-schemas/web_fetch_20260209.json"),
    );
}

#[test]
fn bash_20241022_matches_reference_parsing() {
    check(
        AnthropicTools::new().bash_20241022(),
        include_str!("../fixtures/tool-schemas/bash_20241022.json"),
    );
}

#[test]
fn web_search_20250305_matches_reference_parsing() {
    check(
        AnthropicTools::new().web_search_20250305(Default::default()),
        include_str!("../fixtures/tool-schemas/web_search_20250305.json"),
    );
}

#[test]
fn computer_20241022_matches_reference_parsing() {
    check(
        AnthropicTools::new().computer_20241022(Default::default()),
        include_str!("../fixtures/tool-schemas/computer_20241022.json"),
    );
}

#[test]
fn advisor_20260301_matches_reference_parsing() {
    check(
        AnthropicTools::new().advisor_20260301(ferrin_anthropic::tools::AdvisorArgs {
            model: "claude-sonnet-4-6".into(),
            max_uses: None,
            max_tokens: None,
            caching: None,
        }),
        include_str!("../fixtures/tool-schemas/advisor_20260301.json"),
    );
}

#[test]
fn web_search_20260209_matches_reference_parsing() {
    check(
        AnthropicTools::new().web_search_20260209(Default::default()),
        include_str!("../fixtures/tool-schemas/web_search_20260209.json"),
    );
}

#[test]
fn code_execution_20250825_matches_reference_parsing() {
    check(
        AnthropicTools::new().code_execution_20250825(),
        include_str!("../fixtures/tool-schemas/code_execution_20250825.json"),
    );
}

#[test]
fn code_execution_20260120_matches_reference_parsing() {
    check(
        AnthropicTools::new().code_execution_20260120(),
        include_str!("../fixtures/tool-schemas/code_execution_20260120.json"),
    );
}

#[test]
fn bash_20250124_matches_reference_parsing() {
    check(
        AnthropicTools::new().bash_20250124(),
        include_str!("../fixtures/tool-schemas/bash_20250124.json"),
    );
}

#[test]
fn computer_20250124_matches_reference_parsing() {
    check(
        AnthropicTools::new().computer_20250124(Default::default()),
        include_str!("../fixtures/tool-schemas/computer_20250124.json"),
    );
}

#[test]
fn text_editor_20241022_matches_reference_parsing() {
    check(
        AnthropicTools::new().text_editor_20241022(),
        include_str!("../fixtures/tool-schemas/text_editor_20241022.json"),
    );
}

#[test]
fn text_editor_20250429_matches_reference_parsing() {
    check(
        AnthropicTools::new().text_editor_20250429(),
        include_str!("../fixtures/tool-schemas/text_editor_20250429.json"),
    );
}

#[test]
fn computer_20251124_matches_reference_parsing() {
    check(
        AnthropicTools::new().computer_20251124(Default::default()),
        include_str!("../fixtures/tool-schemas/computer_20251124.json"),
    );
}

#[test]
fn memory_20250818_matches_reference_parsing() {
    check(
        AnthropicTools::new().memory_20250818(),
        include_str!("../fixtures/tool-schemas/memory_20250818.json"),
    );
}

#[test]
fn text_editor_20250124_matches_reference_parsing() {
    check(
        AnthropicTools::new().text_editor_20250124(),
        include_str!("../fixtures/tool-schemas/text_editor_20250124.json"),
    );
}

#[test]
fn tool_search_regex_20251119_matches_reference_parsing() {
    check(
        AnthropicTools::new().tool_search_regex_20251119(),
        include_str!("../fixtures/tool-schemas/tool_search_regex_20251119.json"),
    );
}

#[tokio::test]
async fn all_argument_schemas_match_reference_wire_conversion_and_reject_invalid_options() {
    let test = super::common::TestProvider::start().await;
    let prepare = |id: &str, value: Value| {
        let definition = ferrin_spec::ToolDefinition::Provider {
            id: id.into(),
            name: "registered_tool".into(),
            args: value.as_object().unwrap().clone(),
        };
        let mut options = ferrin_spec::CallOptions::new(vec![]);
        options.tools.push(definition);
        ferrin_anthropic::request::prepare_request(
            test.provider.config(),
            "claude-sonnet-4-6",
            &options,
            false,
            Default::default(),
        )
        .map(|prepared| prepared.body["tools"].clone())
    };
    let cases = [
        include_str!("../fixtures/tool-schemas/text_editor_20250728.json"),
        include_str!("../fixtures/tool-schemas/web_fetch_20250910.json"),
        include_str!("../fixtures/tool-schemas/web_fetch_20260209.json"),
        include_str!("../fixtures/tool-schemas/web_search_20250305.json"),
        include_str!("../fixtures/tool-schemas/advisor_20260301.json"),
        include_str!("../fixtures/tool-schemas/web_search_20260209.json"),
    ];
    for fixture in cases {
        let case: Value = serde_json::from_str(fixture).unwrap();
        let id = case["tool"].as_str().unwrap();
        let args = &case["arguments"];
        assert_eq!(
            prepare(id, args["value"].clone()).unwrap(),
            args["wire"],
            "{id} wire"
        );
        for invalid in args["invalid"].as_array().unwrap() {
            assert!(
                prepare(id, invalid.clone()).is_err(),
                "{id} accepted invalid arguments"
            );
        }
    }
}
