//! Internal parallel wrappers expand only known recipients and replay atomically.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolCallPart;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::openai_options;

fn arguments() -> String {
    json!({"tool_uses":[{"recipient_name":"functions.weather","parameters":{"city":"Paris"}},{"recipient_name":"functions.weather","parameters":{"city":"Tokyo"}}]}).to_string()
}

fn wrapper(input: &str) -> JsonValue {
    json!({"type":"function_call","id":"wrapper_item","call_id":"wrapper","name":"parallel","arguments":input})
}

fn options() -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Weather?")]);
    options.tools = vec![ToolDefinition::function(
        "weather",
        None,
        json!({"type":"object","properties":{"city":{"type":"string"}}}),
    )];
    options
}

async fn expanded() -> Vec<ToolCall> {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/responses",
        Fixture::json(&json!({"output":[wrapper(&arguments())]})),
    );
    test.provider
        .responses("gpt-5")
        .do_generate(options())
        .await
        .unwrap()
        .content
        .into_iter()
        .map(|content| match content {
            Content::ToolCall(call) => call,
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn parallel_wrappers_expand_in_generate_and_stream() {
    let calls = expanded().await;
    let summary: Vec<_> = calls.iter().map(|call| json!({"id":call.tool_call_id,"name":call.tool_name,"input":serde_json::from_str::<JsonValue>(&call.input).unwrap()})).collect();
    assert_eq!(
        summary,
        vec![
            json!({"id":"wrapper_0","name":"weather","input":{"city":"Paris"}}),
            json!({"id":"wrapper_1","name":"weather","input":{"city":"Tokyo"}})
        ]
    );
    let test = TestProvider::start().await;
    test.mount_fixture(Method::POST, "/v1/responses", Fixture::sse_json(&[
        json!({"type":"response.output_item.added","output_index":0,"item":wrapper("")}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":arguments()}),
        json!({"type":"response.output_item.done","output_index":0,"item":wrapper(&arguments())}),
        json!({"type":"response.completed","response":{"output":[]}}),
    ]));
    let parts = collect_checked(
        test.provider
            .responses("gpt-5")
            .do_stream(options())
            .await
            .unwrap(),
    )
    .await;
    let streamed: Vec<_> = parts
        .into_iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, calls);
}

#[tokio::test]
async fn parallel_wrapper_recognition_rejects_unknown_and_explicit_tools() {
    for (input, declared) in [
        (
            json!({"tool_uses":[{"recipient_name":"functions.unknown","parameters":{}}]})
                .to_string(),
            false,
        ),
        (
            json!({"tool_uses":[{"recipient_name":"functions.weather","parameters":[]}]})
                .to_string(),
            false,
        ),
        (arguments(), true),
    ] {
        let test = TestProvider::start().await;
        test.mount_fixture(
            Method::POST,
            "/v1/responses",
            Fixture::json(&json!({"output":[wrapper(&input)]})),
        );
        let mut options = options();
        if declared {
            options.tools.push(ToolDefinition::function(
                "parallel",
                None,
                json!({"type":"object","properties":{}}),
            ));
        }
        let result = test
            .provider
            .responses("gpt-5")
            .do_generate(options)
            .await
            .unwrap();
        let expected = ToolCall {
            provider_metadata: Some(openai_options(json!({"itemId":"wrapper_item"}))),
            ..ToolCall::new("wrapper", "parallel", input)
        };
        assert_eq!(result.content, vec![Content::ToolCall(expected)]);
    }
}

#[tokio::test]
async fn complete_parallel_results_recombine_in_original_order_for_stored_continuations() {
    let calls = expanded().await;
    for state in [
        json!({"previousResponseId":"response_1"}),
        json!({"conversation":"conversation_1"}),
    ] {
        let test = TestProvider::start().await;
        test.mount_fixture(
            Method::POST,
            "/v1/responses",
            Fixture::json(&json!({"output":[]})),
        );
        let mut options = options();
        options.provider_options = openai_options(state.clone());
        options.prompt = vec![
            PromptMessage::Assistant {
                content: calls
                    .iter()
                    .map(|call| {
                        AssistantPromptPart::ToolCall(ToolCallPart {
                            tool_call_id: call.tool_call_id.clone(),
                            tool_name: call.tool_name.clone(),
                            input: serde_json::from_str(&call.input).unwrap(),
                            provider_executed: false,
                            provider_options: call.provider_metadata.clone(),
                        })
                    })
                    .collect(),
                provider_options: None,
            },
            PromptMessage::Tool {
                content: calls
                    .iter()
                    .rev()
                    .map(|call| {
                        ToolPromptPart::ToolResult(ToolResultPart {
                            tool_call_id: call.tool_call_id.clone(),
                            tool_name: call.tool_name.clone(),
                            output: ToolResultOutput::json(json!({"id":call.tool_call_id})),
                            provider_options: call.provider_metadata.clone(),
                        })
                    })
                    .collect(),
                provider_options: None,
            },
        ];
        test.provider
            .responses("gpt-5")
            .do_generate(options)
            .await
            .unwrap();
        let mut expected = vec![];
        if state.get("conversation").is_none() {
            expected.push(json!({"type":"function_call","call_id":"wrapper","name":"parallel","arguments":arguments()}));
        }
        expected.push(json!({"type":"function_call_output","call_id":"wrapper","output":"{\"id\":\"wrapper_0\"}\n{\"id\":\"wrapper_1\"}"}));
        assert_eq!(
            test.only_request().body_json().unwrap()["input"],
            json!(expected)
        );
    }
}
