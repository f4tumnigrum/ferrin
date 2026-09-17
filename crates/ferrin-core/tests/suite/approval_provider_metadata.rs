//! Routing metadata must survive approvals all the way to model-facing messages.

use std::sync::Arc;

use ferrin_core::generate_text;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::step_count;
use ferrin_core::stream_text;
use ferrin_message::Message;
use ferrin_message::ToolApprovalResponse;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::ProviderOptions;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_tool::NeedsApproval;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;

fn metadata() -> ProviderOptions {
    serde_json::from_value(json!({"openai":{"parallelToolCall":{
        "itemId":"wrapper_item","toolCallId":"wrapper","toolName":"parallel",
        "input":"{}","index":0,"count":1
    },"caller":{"type":"direct"}}}))
    .unwrap()
}

fn call() -> ToolCall {
    let mut call = ToolCall::new("wrapper_0", "inspect", "{}");
    call.provider_metadata = Some(metadata());
    call
}

fn tools(outcome: &'static str) -> ToolSet {
    ToolSet::new()
        .insert(
            "inspect",
            Tool::function_with_schema(Schema::empty_object())
                .needs_approval(NeedsApproval::Always)
                .execute(move |input, _| async move {
                    match outcome {
                        "success" => Ok(input),
                        "error" => Err(ToolError::message("unavailable")),
                        _ => panic!("denied tool must never execute"),
                    }
                })
                .build(),
        )
        .unwrap()
}

fn result_metadata(prompt: &[PromptMessage]) -> Vec<Option<ProviderOptions>> {
    prompt
        .iter()
        .flat_map(|message| match message {
            PromptMessage::Tool { content, .. } => content.as_slice(),
            _ => &[],
        })
        .filter_map(|part| match part {
            ToolPromptPart::ToolResult(result) => Some(result.provider_options.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn automatic_denials_keep_routing_in_generate_and_stream() {
    for streaming in [false, true] {
        let model = mock()
            .generate(GenerateResult::new(
                vec![Content::ToolCall(call())],
                FinishReason::tool_calls(),
            ))
            .generate(text_result("denied"))
            .stream(vec![
                StreamPart::stream_start(),
                StreamPart::ToolCall(call()),
                StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
            ])
            .stream(ferrin_testing::text_parts(["denied"], Usage::default()))
            .build_shared();
        if streaming {
            stream_text(Arc::clone(&model))
                .prompt("inspect")
                .tools(tools("denied"))
                .tool_approval(ApprovalStatus::denied())
                .stop_when(step_count(2))
                .await
                .unwrap()
                .consume()
                .await
                .unwrap();
        } else {
            generate_text(Arc::clone(&model))
                .prompt("inspect")
                .tools(tools("denied"))
                .tool_approval(ApprovalStatus::denied())
                .stop_when(step_count(2))
                .await
                .unwrap();
        }
        let calls = if streaming {
            model.stream_calls()
        } else {
            model.generate_calls()
        };
        assert_eq!(result_metadata(&calls[1].prompt), vec![Some(metadata())]);
    }
}

#[tokio::test]
async fn replayed_approval_outcomes_keep_routing_in_generate_and_stream() {
    for outcome in ["success", "error", "denied"] {
        for streaming in [false, true] {
            let first = generate_text(
                mock()
                    .generate(GenerateResult::new(
                        vec![Content::ToolCall(call())],
                        FinishReason::tool_calls(),
                    ))
                    .build_shared(),
            )
            .prompt("inspect")
            .tools(tools(outcome))
            .await
            .unwrap();
            let id = first
                .last_step()
                .tool_approval_requests()
                .next()
                .unwrap()
                .approval_id
                .clone();
            let mut history = vec![Message::user("inspect")];
            history.extend(first.response_messages());
            history.push(Message::tool([if outcome == "denied" {
                ToolApprovalResponse::denied(id)
            } else {
                ToolApprovalResponse::approved(id)
            }]));
            let model = mock()
                .generate(text_result("done"))
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared();
            if streaming {
                stream_text(Arc::clone(&model))
                    .messages(history)
                    .tools(tools(outcome))
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap();
            } else {
                generate_text(Arc::clone(&model))
                    .messages(history)
                    .tools(tools(outcome))
                    .await
                    .unwrap();
            }
            let calls = if streaming {
                model.stream_calls()
            } else {
                model.generate_calls()
            };
            assert_eq!(result_metadata(&calls[0].prompt), vec![Some(metadata())]);
        }
    }
}
