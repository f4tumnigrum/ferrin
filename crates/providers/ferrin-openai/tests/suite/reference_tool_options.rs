//! Function options and replay checked against AI SDK revision 6c6c221.

use ferrin_spec::CallOptions;
use ferrin_spec::PromptMessage;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[tokio::test]
async fn output_schema_normalization_and_scalar_result_encoding_match_reference() {
    let test = TestProvider::start().await;
    let mut function = ToolDefinition::function("lookup", None, json!({"type":"object"}));
    let ToolDefinition::Function {
        provider_options, ..
    } = &mut function
    else {
        panic!("expected function")
    };
    *provider_options = Some(openai_options(json!({
        "outputSchema":{"type":"object","propertyNames":{"type":"string"}}
    })));
    for (output, expected) in [
        (ToolResultOutput::text("hello"), json!("\"hello\"")),
        (ToolResultOutput::error_text("failed"), json!("\"failed\"")),
        (
            ToolResultOutput::ExecutionDenied {
                reason: None,
                provider_options: None,
            },
            json!("\"Tool call execution denied.\""),
        ),
        (
            ToolResultOutput::json(json!({"answer":42})),
            json!("{\"answer\":42}"),
        ),
        (
            ToolResultOutput::error_json(json!("failed")),
            json!("\"failed\""),
        ),
    ] {
        let mut call =
            CallOptions::new(vec![PromptMessage::tool(vec![ToolPromptPart::ToolResult(
                ToolResultPart {
                    tool_call_id: "call".into(),
                    tool_name: "lookup".into(),
                    output,
                    provider_options: None,
                },
            )])]);
        call.tools.push(function.clone());
        let request = ferrin_openai::responses::request::prepare_request(
            test.provider.config(),
            "gpt-6",
            &call,
        )
        .unwrap();
        let body = serde_json::to_value(request.body).unwrap();
        assert_eq!(
            (
                body["tools"][0]["output_schema"].clone(),
                body["input"].clone()
            ),
            (
                json!({"type":"object"}),
                json!([{"type":"function_call_output","call_id":"call","output":expected}])
            ),
        );
        assert_eq!(request.warnings.len(), 1);
    }
}

#[tokio::test]
async fn namespaces_keep_first_declaration_order_and_empty_tools_drop_choice() {
    let test = TestProvider::start().await;
    let function = |name: &str, namespace: bool| {
        let mut tool = ToolDefinition::function(name, None, json!({"type":"object"}));
        if namespace
            && let ToolDefinition::Function {
                provider_options, ..
            } = &mut tool
        {
            *provider_options = Some(openai_options(
                json!({"namespace":{"name":"group","description":"Grouped tools"}}),
            ));
        }
        tool
    };
    let mut call = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    call.tools = vec![
        function("first", true),
        function("middle", false),
        function("last", true),
    ];
    let request =
        ferrin_openai::responses::request::prepare_request(test.provider.config(), "gpt-6", &call)
            .unwrap();
    let body = serde_json::to_value(request.body).unwrap();
    assert_eq!(
        body["tools"],
        json!([
            {"type":"namespace","name":"group","description":"Grouped tools","tools":[
                {"type":"function","name":"first","parameters":{"type":"object"}},
                {"type":"function","name":"last","parameters":{"type":"object"}}
            ]},
            {"type":"function","name":"middle","parameters":{"type":"object"}}
        ])
    );
    call.tools.clear();
    call.tool_choice = Some(ferrin_spec::ToolChoice::Required);
    call.provider_options = openai_options(json!({"allowedTools":{"toolNames":["missing"]}}));
    let request =
        ferrin_openai::responses::request::prepare_request(test.provider.config(), "gpt-6", &call)
            .unwrap();
    let body = serde_json::to_value(request.body).unwrap();
    assert_eq!(
        (body.get("tools"), body.get("tool_choice"), request.warnings),
        (None, None, vec![])
    );
}
