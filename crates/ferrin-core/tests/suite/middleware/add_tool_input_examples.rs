use ferrin_core::middleware::builtin::add_tool_input_examples;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::options;

fn tool(description: Option<&str>, examples: &[serde_json::Value]) -> ToolDefinition {
    ToolDefinition::Function {
        name: "t".into(),
        description: description.map(str::to_owned),
        input_schema: json!({ "type": "object" }),
        strict: None,
        input_examples: examples
            .iter()
            .map(|example| example.as_object().unwrap().clone())
            .collect(),
        provider_options: None,
    }
}

fn description_and_examples(tool: &ToolDefinition) -> (Option<String>, usize) {
    match tool {
        ToolDefinition::Function {
            description,
            input_examples,
            ..
        } => (description.clone(), input_examples.len()),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn appends_examples_and_removes_them_by_default() {
    let mut call = options();
    call.tools = vec![
        tool(
            Some("Adds numbers."),
            &[json!({ "a": 1, "b": 2 }), json!({ "a": 3 })],
        ),
        tool(None, &[json!({ "x": true })]),
        tool(Some("No examples."), &[]),
    ];
    let result = add_tool_input_examples().apply(call);
    assert_eq!(
        description_and_examples(&result.tools[0]),
        (
            Some("Adds numbers.\n\nInput Examples:\n{\"a\":1,\"b\":2}\n{\"a\":3}".to_owned()),
            0
        )
    );
    assert_eq!(
        description_and_examples(&result.tools[1]),
        (Some("Input Examples:\n{\"x\":true}".to_owned()), 0)
    );
    assert_eq!(
        description_and_examples(&result.tools[2]),
        (Some("No examples.".to_owned()), 0)
    );
}

#[test]
fn custom_prefix_format_and_keep() {
    let mut call = options();
    call.tools = vec![tool(Some("Desc"), &[json!({ "a": 1 }), json!({ "a": 2 })])];
    let result = add_tool_input_examples()
        .prefix("Examples:")
        .format(|example, index| format!("{}. a={}", index + 1, example["a"]))
        .remove(false)
        .apply(call);
    assert_eq!(
        description_and_examples(&result.tools[0]),
        (Some("Desc\n\nExamples:\n1. a=1\n2. a=2".to_owned()), 2)
    );
}

#[test]
fn empty_descriptions_do_not_add_leading_blank_lines() {
    let mut call = options();
    call.tools = vec![
        tool(Some(""), &[json!({"value":1})]),
        tool(None, &[json!({"value":1})]),
    ];
    let transformed = add_tool_input_examples().apply(call);
    assert_eq!(transformed.tools[0], transformed.tools[1]);
    assert_eq!(
        description_and_examples(&transformed.tools[0]),
        (Some("Input Examples:\n{\"value\":1}".into()), 0)
    );
}
