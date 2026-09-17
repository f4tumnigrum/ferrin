//! Denied child calls still replay as one stored OpenAI parallel wrapper.

use std::sync::Arc;

use ferrin::generate_text::ApprovalStatus;
use ferrin::openai::responses::request::prepare_request;
use ferrin::prelude::*;
use ferrin::spec::ToolCall;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;

fn wrapper_input() -> String {
    json!({"tool_uses":[
        {"recipient_name":"functions.inspect","parameters":{"number":0}},
        {"recipient_name":"functions.inspect","parameters":{"number":1}}
    ]})
    .to_string()
}

fn calls() -> Vec<ToolCall> {
    (0..2)
        .map(|index| {
            let mut call = ToolCall::new(
                format!("wrapper_{index}"),
                "inspect",
                json!({"number":index}).to_string(),
            );
            call.provider_metadata = Some(
                serde_json::from_value(json!({"openai":{"parallelToolCall":{
                    "itemId":"wrapper_item","toolCallId":"wrapper","toolName":"parallel",
                    "input":wrapper_input(),"index":index,"count":2
                }}}))
                .unwrap(),
            );
            call
        })
        .collect()
}

fn tools() -> ToolSet {
    ToolSet::new()
        .insert(
            "inspect",
            Tool::function_with_schema(Schema::from_json_schema(json!({
                "type":"object","properties":{"number":{"type":"integer"}},"required":["number"]
            })))
            .needs_approval(NeedsApproval::Always)
            .execute(|_: JsonValue, _| async {
                Err::<JsonValue, _>(ToolError::message("denied tool must never execute"))
            })
            .build(),
        )
        .unwrap()
}

#[tokio::test]
async fn denied_parallel_children_replay_as_one_wrapper_after_auto_and_user_approval() {
    let provider =
        ferrin::openai::create_openai(ferrin::openai::OpenAiSettings::default()).unwrap();
    for streaming in [false, true] {
        for replay in [false, true] {
            let first_content = calls().into_iter().map(Content::ToolCall).collect();
            let first_parts = std::iter::once(StreamPart::stream_start())
                .chain(calls().into_iter().map(StreamPart::ToolCall))
                .chain([StreamPart::finish(
                    FinishReason::tool_calls(),
                    Usage::default(),
                )])
                .collect::<Vec<_>>();
            let model = MockLanguageModel::builder()
                .generate(GenerateResult::new(
                    first_content,
                    FinishReason::tool_calls(),
                ))
                .generate(GenerateResult::new(
                    vec![Content::text("denied")],
                    FinishReason::stop(),
                ))
                .stream(first_parts)
                .stream(ferrin_testing::text_parts(["denied"], Usage::default()))
                .build_shared();
            let policy = if replay {
                ApprovalStatus::user_approval()
            } else {
                ApprovalStatus::denied()
            };
            let first = if streaming {
                stream_text(Arc::clone(&model))
                    .prompt("inspect")
                    .tools(tools())
                    .tool_approval(policy)
                    .stop_when(step_count(2))
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                generate_text(Arc::clone(&model))
                    .prompt("inspect")
                    .tools(tools())
                    .tool_approval(policy)
                    .stop_when(step_count(2))
                    .await
                    .unwrap()
            };
            if replay {
                let responses = first
                    .last_step()
                    .tool_approval_requests()
                    .map(|request| ToolApprovalResponse::denied(request.approval_id.clone()))
                    .collect::<Vec<_>>();
                let mut history = vec![Message::user("inspect")];
                history.extend(first.response_messages());
                history.push(Message::tool(responses));
                if streaming {
                    stream_text(Arc::clone(&model))
                        .messages(history)
                        .tools(tools())
                        .await
                        .unwrap()
                        .consume()
                        .await
                        .unwrap();
                } else {
                    generate_text(Arc::clone(&model))
                        .messages(history)
                        .tools(tools())
                        .await
                        .unwrap();
                }
            }
            let requests = if streaming {
                model.stream_calls()
            } else {
                model.generate_calls()
            };
            for state in [
                json!({"previousResponseId":"response_1"}),
                json!({"conversation":"conversation_1"}),
            ] {
                let mut request = requests[1].clone();
                request.provider_options = serde_json::from_value(json!({"openai":state})).unwrap();
                let input = prepare_request(provider.config(), "gpt-5", &request)
                    .unwrap()
                    .body
                    .input;
                let function_items: Vec<_> = input
                    .into_iter()
                    .filter(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call" | "function_call_output")
                        )
                    })
                    .collect();
                let mut expected = vec![];
                if state.get("conversation").is_none() {
                    expected.push(json!({"type":"function_call","call_id":"wrapper","name":"parallel","arguments":wrapper_input()}));
                }
                expected.push(json!({"type":"function_call_output","call_id":"wrapper","output":"Tool call execution denied.\nTool call execution denied."}));
                assert_eq!(function_items, expected);
            }
        }
    }
}
