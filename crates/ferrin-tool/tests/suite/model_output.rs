use std::time::Duration;

use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_tool::ErrorMode;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolError;
use ferrin_tool::model_output::create_tool_model_output;
use ferrin_tool::model_output::error_message;
use ferrin_tool::model_output::tool_error_output;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn default_conversion() {
    let id = "call_1".into();
    assert_eq!(
        create_tool_model_output(None, &id, &json!({}), &json!("hello"), ErrorMode::None),
        ToolResultOutput::text("hello")
    );
    assert_eq!(
        create_tool_model_output(None, &id, &json!({}), &json!({ "a": 1 }), ErrorMode::None),
        ToolResultOutput::json(json!({ "a": 1 }))
    );
    assert_eq!(
        create_tool_model_output(
            None,
            &id,
            &json!({}),
            &json!({ "message": "bad" }),
            ErrorMode::Text
        ),
        ToolResultOutput::error_text("bad")
    );
    assert_eq!(
        create_tool_model_output(None, &id, &json!({}), &json!([1]), ErrorMode::Json),
        ToolResultOutput::error_json(json!([1]))
    );
}

#[test]
fn custom_conversion_receives_call_details() {
    let tool = Tool::function_with_schema(Schema::empty_object())
        .to_model_output(|args| {
            ToolResultOutput::text(format!(
                "{}:{}:{}",
                args.tool_call_id,
                args.input["q"].as_str().unwrap_or(""),
                args.output
            ))
        })
        .build();
    assert_eq!(
        create_tool_model_output(
            Some(&tool),
            &"call_9".into(),
            &json!({ "q": "x" }),
            &json!(42),
            ErrorMode::None
        ),
        ToolResultOutput::text("call_9:x:42")
    );
}

#[test]
fn error_rendering() {
    assert_eq!(
        tool_error_output(&ToolError::message("boom")),
        ToolResultOutput::error_text("boom")
    );
    assert_eq!(
        tool_error_output(&ToolError::json(json!({ "code": 1 }))),
        ToolResultOutput::error_json(json!({ "code": 1 }))
    );
    assert_eq!(
        tool_error_output(&ToolError::Timeout(Duration::from_secs(2))),
        ToolResultOutput::error_text("tool execution timed out after 2s")
    );
    assert_eq!(error_message(&json!(null)), "unknown error");
    assert_eq!(error_message(&json!("text")), "text");
    assert_eq!(error_message(&json!({ "message": "m" })), "m");
    assert_eq!(error_message(&json!({ "code": 1 })), "{\"code\":1}");
}
