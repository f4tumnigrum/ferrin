//! Advanced hosted/client tool protocol roundtrips.

use ferrin_openai::tools::OpenAiTools;
use ferrin_openai::tools::ShellArgs;
use ferrin_openai::tools::ToolSearchArgs;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::openai_options;

fn definitions() -> Vec<ToolDefinition> {
    let tools = OpenAiTools::new();
    vec![
        tools
            .programmatic_tool_calling()
            .definition("program".into(), None),
        tools
            .tool_search(ToolSearchArgs::default())
            .definition("search".into(), None),
        tools
            .shell(ShellArgs {
                environment: Some(json!({"type": "containerAuto"})),
            })
            .definition("terminal".into(), None),
    ]
}

fn options() -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Run the tools")]);
    options.tools = definitions();
    options
}

pub(super) fn advanced_output() -> Vec<JsonValue> {
    vec![
        json!({"type":"program","id":"prog_1","call_id":"program_1","code":"await tools.weather({});","fingerprint":"fp_1"}),
        json!({"type":"function_call","id":"fn_1","call_id":"weather_1","name":"weather","arguments":"{}","caller":{"type":"program","caller_id":"program_1"}}),
        json!({"type":"program_output","id":"prog_out_1","call_id":"program_1","result":"sunny","status":"completed"}),
        json!({"type":"tool_search_call","id":"search_1","execution":"server","call_id":null,"arguments":{"query":"weather"}}),
        json!({"type":"tool_search_output","id":"search_out_1","execution":"server","call_id":null,"tools":[{"type":"function","name":"weather"}]}),
        json!({"type":"shell_call","id":"shell_1","call_id":"shell_call_1","action":{"commands":["pwd"],"timeout_ms":1000,"max_output_length":100}}),
        json!({"type":"shell_call_output","id":"shell_out_1","call_id":"shell_call_1","output":[{"stdout":"/work","stderr":"","outcome":{"type":"exit","exit_code":0}}]}),
    ]
}

fn response(output: Vec<JsonValue>) -> JsonValue {
    json!({"id":"resp_advanced","model":"gpt-5","output":output,"usage":{"input_tokens":1,"output_tokens":1}})
}

fn expected() -> Vec<JsonValue> {
    vec![
        json!({"call":"program_1","name":"program","input":{"code":"await tools.weather({});","fingerprint":"fp_1"},"hosted":true,"meta":{"openai":{"itemId":"prog_1"}}}),
        json!({"call":"weather_1","name":"weather","input":{},"hosted":false,"meta":{"openai":{"itemId":"fn_1","caller":{"type":"program","callerId":"program_1"}}}}),
        json!({"result":"program_1","name":"program","output":{"result":"sunny","status":"completed"},"meta":{"openai":{"itemId":"prog_out_1"}}}),
        json!({"call":"search_1","name":"search","input":{"arguments":{"query":"weather"},"call_id":null},"hosted":true,"meta":{"openai":{"itemId":"search_1"}}}),
        json!({"result":"search_1","name":"search","output":{"tools":[{"type":"function","name":"weather"}]},"meta":{"openai":{"itemId":"search_out_1"}}}),
        json!({"call":"shell_call_1","name":"terminal","input":{"action":{"commands":["pwd"],"timeoutMs":1000,"maxOutputLength":100}},"hosted":true,"meta":{"openai":{"itemId":"shell_1"}}}),
        json!({"result":"shell_call_1","name":"terminal","output":{"output":[{"stdout":"/work","stderr":"","outcome":{"type":"exit","exitCode":0}}]},"meta":{"openai":{"itemId":"shell_out_1"}}}),
    ]
}

fn summarize(content: &[Content]) -> Vec<JsonValue> {
    content.iter().filter_map(|part| match part {
        Content::ToolCall(call) => Some(json!({"call":call.tool_call_id,"name":call.tool_name,"input":serde_json::from_str::<JsonValue>(&call.input).unwrap(),"hosted":call.provider_executed,"meta":call.provider_metadata})),
        Content::ToolResult(result) => Some(json!({"result":result.tool_call_id,"name":result.tool_name,"output":result.result,"meta":result.provider_metadata})),
        _ => None,
    }).collect()
}

#[tokio::test]
async fn hosted_tools_map_identically_in_generate_and_stream() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/responses",
        Fixture::json(&response(advanced_output())),
    );
    let generated = test
        .provider
        .responses("gpt-5")
        .do_generate(options())
        .await
        .unwrap();
    assert_eq!(summarize(&generated.content), expected());

    let test = TestProvider::start().await;
    let mut events = Vec::new();
    for (index, item) in advanced_output().iter().enumerate() {
        for kind in ["response.output_item.added", "response.output_item.done"] {
            events.push(format!(
                "data: {}\n\n",
                json!({"type":kind,"output_index":index,"item":item})
            ));
        }
    }
    events.push(format!(
        "data: {}\n\n",
        json!({"type":"response.completed","response":response(Vec::new())})
    ));
    test.mount_fixture(Method::POST, "/v1/responses", Fixture::sse(events));
    let parts = collect_checked(
        test.provider
            .responses("gpt-5")
            .do_stream(options())
            .await
            .unwrap(),
    )
    .await;
    let content: Vec<_> = parts
        .into_iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some(Content::ToolCall(call)),
            StreamPart::ToolResult(result) => Some(Content::ToolResult(result)),
            _ => None,
        })
        .collect();
    assert_eq!(summarize(&content), expected());
}

#[tokio::test]
async fn advanced_provider_items_replay_without_storage() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/responses",
        Fixture::json(&response(advanced_output())),
    );
    let generated = test
        .provider
        .responses("gpt-5")
        .do_generate(options())
        .await
        .unwrap();
    let content = generated
        .content
        .into_iter()
        .map(|part| match part {
            Content::ToolCall(call) => AssistantPromptPart::ToolCall(ToolCallPart {
                tool_call_id: call.tool_call_id,
                tool_name: call.tool_name,
                input: serde_json::from_str(&call.input).unwrap(),
                provider_executed: call.provider_executed,
                provider_options: call.provider_metadata,
            }),
            Content::ToolResult(result) => AssistantPromptPart::ToolResult(ToolResultPart {
                tool_call_id: result.tool_call_id,
                tool_name: result.tool_name,
                output: ToolResultOutput::json(result.result),
                provider_options: result.provider_metadata,
            }),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/responses",
        Fixture::json(&response(Vec::new())),
    );
    let mut options = options();
    options.prompt = vec![PromptMessage::Assistant {
        content,
        provider_options: None,
    }];
    options.provider_options = openai_options(json!({"store":false}));
    test.provider
        .responses("gpt-5")
        .do_generate(options)
        .await
        .unwrap();
    let input = test.only_request().body_json().unwrap()["input"].clone();
    let mut expected = advanced_output();
    expected[3]["status"] = json!("completed");
    expected[4]["status"] = json!("completed");
    expected[5]["status"] = json!("completed");
    expected[6].as_object_mut().unwrap().remove("id");
    assert_eq!(input, json!(expected));
}

#[tokio::test]
async fn client_search_and_program_callee_results_keep_their_wire_types() {
    use ferrin_spec::language_model::prompt::ToolPromptPart;
    let test = TestProvider::start().await;
    test.mount_fixture(Method::POST,"/v1/responses",Fixture::json(&json!({"output":[
        {"type":"tool_search_call","id":"search_item","execution":"client","call_id":"search_call","arguments":{"query":"weather"}}
    ]})));
    let mut request = options();
    request.tools = vec![
        OpenAiTools::new()
            .tool_search(ToolSearchArgs {
                execution: Some("client".into()),
                ..ToolSearchArgs::default()
            })
            .definition("search".into(), None),
    ];
    let result = test
        .provider
        .responses("gpt-5")
        .do_generate(request.clone())
        .await
        .unwrap();
    let mut expected = ferrin_spec::ToolCall::new(
        "search_call",
        "search",
        json!({"arguments":{"query":"weather"},"call_id":"search_call"}).to_string(),
    );
    expected.provider_metadata = Some(openai_options(json!({"itemId":"search_item"})));
    assert_eq!(result.content, vec![Content::ToolCall(expected)]);
    assert_eq!(
        result.finish_reason.unified,
        ferrin_spec::FinishReasonKind::ToolCalls
    );

    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/responses",
        Fixture::json(&json!({"output":[]})),
    );
    request.prompt = vec![PromptMessage::Tool {
        content: vec![
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "search_call".into(),
                tool_name: "search".into(),
                output: ToolResultOutput::json(
                    json!({"tools":[{"type":"function","name":"weather"}]}),
                ),
                provider_options: None,
            }),
            ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "weather_1".into(),
                tool_name: "weather".into(),
                output: ToolResultOutput::json(json!({"temperature":21})),
                provider_options: Some(openai_options(
                    json!({"caller":{"type":"program","callerId":"program_1"}}),
                )),
            }),
        ],
        provider_options: None,
    }];
    test.provider
        .responses("gpt-5")
        .do_generate(request)
        .await
        .unwrap();
    assert_eq!(
        test.only_request().body_json().unwrap()["input"],
        json!([
            {"type":"tool_search_output","execution":"client","call_id":"search_call","status":"completed","tools":[{"type":"function","name":"weather"}]},
            {"type":"function_call_output","call_id":"weather_1","output":"{\"temperature\":21}","caller":{"type":"program","caller_id":"program_1"}}
        ])
    );
}

#[tokio::test]
async fn batch_maps_program_search_and_hosted_shell_output() {
    use ferrin_openai::batch::results::convert_line;
    use ferrin_spec::batch::BatchItem;
    use ferrin_spec::batch::BatchItemResult;
    let test = TestProvider::start().await;
    let item = convert_line(test.provider.config(),serde_json::from_value(json!({"custom_id":"advanced","response":{"status_code":200,"body":response(advanced_output())}})).unwrap());
    let BatchItemResult::Text(item) = item else {
        panic!("unexpected batch item {item:?}");
    };
    let BatchItem::Succeeded { result, .. } = *item else {
        panic!("unexpected text item {item:?}");
    };
    let mut expected = expected();
    for part in &mut expected {
        if part["name"] == "program" {
            part["name"] = json!("programmatic_tool_calling");
        }
        if part["name"] == "search" {
            part["name"] = json!("tool_search");
        }
        if part["name"] == "terminal" {
            part["name"] = json!("shell");
        }
    }
    assert_eq!(summarize(&result.content), expected);
}

#[tokio::test]
async fn program_callee_denials_are_rejected_even_without_result_metadata() {
    use ferrin_spec::language_model::prompt::ToolPromptPart;
    let test = TestProvider::start().await;
    let mut request = options();
    request.prompt = vec![
        PromptMessage::Assistant {
            content: vec![AssistantPromptPart::ToolCall(ToolCallPart {
                tool_call_id: "callee".into(),
                tool_name: "weather".into(),
                input: json!({}),
                provider_executed: false,
                provider_options: Some(openai_options(
                    json!({"caller":{"type":"program","callerId":"program_1"}}),
                )),
            })],
            provider_options: None,
        },
        PromptMessage::Tool {
            content: vec![ToolPromptPart::ToolResult(ToolResultPart {
                tool_call_id: "callee".into(),
                tool_name: "weather".into(),
                output: ToolResultOutput::ExecutionDenied {
                    reason: None,
                    provider_options: None,
                },
                provider_options: None,
            })],
            provider_options: None,
        },
    ];
    let error = test
        .provider
        .responses("gpt-5")
        .do_generate(request)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ferrin_spec::error::ProviderError::UnsupportedFunctionality(_)
    ));
    assert!(test.server.received().is_empty());
}
