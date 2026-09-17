//! Every existing provider factory checked against reference Zod parsing fixtures.

use ferrin_openai::tools::OpenAiTools;
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
fn web_search_matches_reference_parsing() {
    check(
        OpenAiTools::new().web_search(Default::default()),
        include_str!("../fixtures/tool-schemas/web_search.json"),
    );
}

#[test]
fn web_search_preview_matches_reference_parsing() {
    check(
        OpenAiTools::new().web_search_preview(Default::default()),
        include_str!("../fixtures/tool-schemas/web_search_preview.json"),
    );
}

#[test]
fn code_interpreter_matches_reference_parsing() {
    check(
        OpenAiTools::new().code_interpreter(Default::default()),
        include_str!("../fixtures/tool-schemas/code_interpreter.json"),
    );
}

#[test]
fn apply_patch_matches_reference_parsing() {
    check(
        OpenAiTools::new().apply_patch(),
        include_str!("../fixtures/tool-schemas/apply_patch.json"),
    );
}

#[test]
fn local_shell_matches_reference_parsing() {
    check(
        OpenAiTools::new().local_shell(),
        include_str!("../fixtures/tool-schemas/local_shell.json"),
    );
}

#[test]
fn programmatic_tool_calling_matches_reference_parsing() {
    check(
        OpenAiTools::new().programmatic_tool_calling(),
        include_str!("../fixtures/tool-schemas/programmatic_tool_calling.json"),
    );
}

#[test]
fn mcp_matches_reference_parsing() {
    check(
        OpenAiTools::new().mcp(Default::default()),
        include_str!("../fixtures/tool-schemas/mcp.json"),
    );
}

#[test]
fn computer_matches_reference_parsing() {
    check(
        OpenAiTools::new().computer(),
        include_str!("../fixtures/tool-schemas/computer.json"),
    );
}

#[test]
fn shell_matches_reference_parsing() {
    check(
        OpenAiTools::new().shell(Default::default()),
        include_str!("../fixtures/tool-schemas/shell.json"),
    );
}

#[test]
fn file_search_matches_reference_parsing() {
    check(
        OpenAiTools::new().file_search(Default::default()),
        include_str!("../fixtures/tool-schemas/file_search.json"),
    );
}

#[test]
fn custom_matches_reference_parsing() {
    check(
        OpenAiTools::new().custom(Default::default()),
        include_str!("../fixtures/tool-schemas/custom.json"),
    );
}

#[test]
fn image_generation_matches_reference_parsing() {
    check(
        OpenAiTools::new().image_generation(Default::default()),
        include_str!("../fixtures/tool-schemas/image_generation.json"),
    );
}

#[test]
fn tool_search_matches_reference_parsing() {
    check(
        OpenAiTools::new().tool_search(Default::default()),
        include_str!("../fixtures/tool-schemas/tool_search.json"),
    );
}

#[tokio::test]
async fn all_argument_schemas_match_reference_wire_conversion_and_reject_invalid_options() {
    let prepare = |id: &str, value: Value| {
        let definition = ferrin_spec::ToolDefinition::Provider {
            id: id.into(),
            name: "registered_tool".into(),
            args: value.as_object().unwrap().clone(),
        };
        let mapping = ferrin_openai::responses::convert_tools::tool_name_mapping(
            std::slice::from_ref(&definition),
        );
        ferrin_openai::responses::convert_tools::convert_tools(
            &[definition],
            None,
            &mapping,
            true,
            "openai",
        )
        .map(|prepared| serde_json::to_value(prepared.tools).unwrap())
    };
    let cases = [
        include_str!("../fixtures/tool-schemas/web_search.json"),
        include_str!("../fixtures/tool-schemas/web_search_preview.json"),
        include_str!("../fixtures/tool-schemas/code_interpreter.json"),
        include_str!("../fixtures/tool-schemas/apply_patch.json"),
        include_str!("../fixtures/tool-schemas/mcp.json"),
        include_str!("../fixtures/tool-schemas/shell.json"),
        include_str!("../fixtures/tool-schemas/file_search.json"),
        include_str!("../fixtures/tool-schemas/custom.json"),
        include_str!("../fixtures/tool-schemas/image_generation.json"),
        include_str!("../fixtures/tool-schemas/tool_search.json"),
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
