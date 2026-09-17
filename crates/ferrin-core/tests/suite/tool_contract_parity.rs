//! Reference SDK parity for dynamic provider calls, named contexts and approval callbacks.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::generate_text;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::approval_policy;
use ferrin_core::stream_text;
use ferrin_message::Message;
use ferrin_message::ToolApprovalResponse;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_tool::NeedsApproval;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;

#[tokio::test]
async fn unknown_dynamic_provider_calls_parse_and_refine_with_any_tool_set() {
    for streaming in [false, true] {
        for registered in [false, true] {
            for provider_executed in [false, true] {
                let mut call = ToolCall::new("remote_1", "mcp.inspect", "{}");
                call.dynamic = true;
                call.provider_executed = provider_executed;
                let model = mock()
                    .generate(GenerateResult::new(
                        vec![Content::ToolCall(call.clone())],
                        FinishReason::stop(),
                    ))
                    .stream(vec![
                        StreamPart::stream_start(),
                        StreamPart::ToolCall(call),
                        StreamPart::finish(FinishReason::stop(), Usage::default()),
                    ])
                    .build_shared();
                let tools = if registered {
                    ToolSet::new()
                        .insert(
                            "local",
                            Tool::function_with_schema(Schema::empty_object()).build(),
                        )
                        .unwrap()
                } else {
                    ToolSet::new()
                };
                let refine = |mut input: JsonValue| {
                    Box::pin(async move {
                        input["refined"] = json!(true);
                        Ok(input)
                    })
                        as ferrin_spec::BoxFuture<'static, Result<JsonValue, ferrin_core::Error>>
                };
                let result = if streaming {
                    stream_text(model)
                        .prompt("inspect")
                        .tools(tools)
                        .refine_tool_input("mcp.inspect", refine)
                        .await
                        .unwrap()
                        .consume()
                        .await
                        .unwrap()
                } else {
                    generate_text(model)
                        .prompt("inspect")
                        .tools(tools)
                        .refine_tool_input("mcp.inspect", refine)
                        .await
                        .unwrap()
                };
                let call = result.last_step().tool_calls().next().unwrap();
                assert_eq!(
                    (
                        call.invalid,
                        call.dynamic,
                        call.provider_executed,
                        call.input.clone()
                    ),
                    (
                        !provider_executed,
                        true,
                        provider_executed,
                        if provider_executed {
                            json!({"refined":true})
                        } else {
                            json!({})
                        }
                    )
                );
            }
        }
    }
}

#[tokio::test]
async fn named_contexts_reach_only_the_matching_description_approval_and_executor() {
    for streaming in [false, true] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut tools = ToolSet::new();
        for (name, schema) in [
            ("text", Some(json!({"type":"string"}))),
            ("number", Some(json!({"type":"integer"}))),
            ("raw", None),
        ] {
            let describe = seen.clone();
            let approve = seen.clone();
            let execute = seen.clone();
            let mut tool = Tool::function_with_schema(Schema::empty_object())
                .description_fn(move |ctx| {
                    describe
                        .lock()
                        .unwrap()
                        .push((name, "description", ctx.tool_context));
                    async { "inspect".to_owned() }
                })
                .needs_approval_if(move |_, ctx| {
                    approve
                        .lock()
                        .unwrap()
                        .push((name, "approval", ctx.tools_context));
                    async { false }
                })
                .execute(move |_, ctx| {
                    execute
                        .lock()
                        .unwrap()
                        .push((name, "execute", ctx.tools_context.clone()));
                    async move { Ok::<_, ToolError>(ctx.tools_context) }
                });
            if let Some(schema) = schema {
                tool = tool.context_schema(Schema::from_json_schema(schema));
            }
            tools.try_insert(name, tool.build()).unwrap();
        }
        let calls: Vec<_> = ["text", "number", "raw"]
            .into_iter()
            .map(|name| ToolCall::new(name, name, "{}"))
            .collect();
        let model = mock()
            .generate(GenerateResult::new(
                calls.iter().cloned().map(Content::ToolCall).collect(),
                FinishReason::tool_calls(),
            ))
            .stream(
                std::iter::once(StreamPart::stream_start())
                    .chain(calls.into_iter().map(StreamPart::ToolCall))
                    .chain([StreamPart::finish(
                        FinishReason::tool_calls(),
                        Usage::default(),
                    )])
                    .collect::<Vec<_>>(),
            )
            .build_shared();
        let contexts = json!({"text":"only-text","number":42,"raw":{"tenant":"only-raw"},"unused":{"private":true}});
        if streaming {
            stream_text(model)
                .prompt("inspect")
                .tools(tools)
                .tools_context(contexts)
                .await
                .unwrap()
                .consume()
                .await
                .unwrap();
        } else {
            generate_text(model)
                .prompt("inspect")
                .tools(tools)
                .tools_context(contexts)
                .await
                .unwrap();
        }
        let mut actual = seen.lock().unwrap().clone();
        actual.sort_by_key(|(name, stage, _)| (*name, *stage));
        let mut expected = Vec::new();
        for (name, value) in [
            ("text", json!("only-text")),
            ("number", json!(42)),
            ("raw", json!({"tenant":"only-raw"})),
        ] {
            for stage in ["description", "approval", "execute"] {
                expected.push((name, stage, Some(value.clone())));
            }
        }
        expected.sort_by_key(|(name, stage, _)| (*name, *stage));
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn absent_policy_and_unconfigured_map_keep_tool_approval_but_callback_none_overrides_it() {
    for streaming in [false, true] {
        for mode in ["absent", "unconfigured", "none", "not-applicable"] {
            let tool = Tool::function_with_schema(Schema::empty_object())
                .needs_approval(NeedsApproval::Always)
                .execute(|_, _| async { Ok::<_, ToolError>(json!("executed")) })
                .build();
            let tools = ToolSet::new().insert("inspect", tool).unwrap();
            let call = ToolCall::new("call", "inspect", "{}");
            let model = mock()
                .generate(GenerateResult::new(
                    vec![Content::ToolCall(call.clone())],
                    FinishReason::tool_calls(),
                ))
                .stream(vec![
                    StreamPart::stream_start(),
                    StreamPart::ToolCall(call),
                    StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
                ])
                .build_shared();
            let result = if streaming {
                let call = stream_text(model).prompt("inspect").tools(tools);
                let call = match mode {
                    "none" => call.tool_approval(approval_policy(|_, _| None)),
                    "not-applicable" => call.tool_approval(ApprovalStatus::NotApplicable),
                    "unconfigured" => {
                        call.tool_approval(HashMap::<ToolName, ApprovalStatus>::new())
                    }
                    _ => call,
                };
                call.await.unwrap().consume().await.unwrap()
            } else {
                let call = generate_text(model).prompt("inspect").tools(tools);
                let call = match mode {
                    "none" => call.tool_approval(approval_policy(|_, _| None)),
                    "not-applicable" => call.tool_approval(ApprovalStatus::NotApplicable),
                    "unconfigured" => {
                        call.tool_approval(HashMap::<ToolName, ApprovalStatus>::new())
                    }
                    _ => call,
                };
                call.await.unwrap()
            };
            let should_execute = matches!(mode, "none" | "not-applicable");
            assert_eq!(
                (
                    result.last_step().tool_results().count(),
                    result.last_step().tool_approval_requests().count()
                ),
                (usize::from(should_execute), usize::from(!should_execute))
            );
        }
    }
}

#[tokio::test]
async fn named_replay_contexts_are_validated_together_before_side_effects() {
    for streaming in [false, true] {
        for contexts in [
            json!({"text":"selected","number":42}),
            json!({"text":"selected","number":"wrong type"}),
            json!({"text":"selected"}),
        ] {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let mut tools = ToolSet::new();
            for (name, kind) in [("text", "string"), ("number", "integer")] {
                let seen = Arc::clone(&seen);
                tools
                    .try_insert(
                        name,
                        Tool::function_with_schema(Schema::empty_object())
                            .context_schema(Schema::from_json_schema(json!({"type":kind})))
                            .needs_approval(NeedsApproval::Always)
                            .execute(move |_, ctx| {
                                seen.lock().unwrap().push((name, ctx.tools_context));
                                async { Ok::<_, ToolError>(json!("executed")) }
                            })
                            .build(),
                    )
                    .unwrap();
            }
            let model = mock()
                .generate(GenerateResult::new(
                    vec![
                        Content::ToolCall(ToolCall::new("text_call", "text", "{}")),
                        Content::ToolCall(ToolCall::new("number_call", "number", "{}")),
                    ],
                    FinishReason::tool_calls(),
                ))
                .build_shared();
            let first = generate_text(model)
                .prompt("inspect both")
                .tools(tools.clone())
                .tools_context(json!({"text":"initial","number":1}))
                .await
                .unwrap();
            let approvals = first
                .last_step()
                .tool_approval_requests()
                .map(|request| ToolApprovalResponse::approved(request.approval_id.clone()));
            let mut history = vec![Message::user("inspect both")];
            history.extend(first.response_messages());
            history.push(Message::tool(approvals));
            let model = mock()
                .generate(GenerateResult::new(
                    vec![Content::text("done")],
                    FinishReason::stop(),
                ))
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared();
            let valid = contexts["number"] == json!(42);
            let result = if streaming {
                match stream_text(Arc::clone(&model))
                    .messages(history)
                    .tools(tools)
                    .tools_context(contexts)
                    .await
                {
                    Ok(stream) => stream.consume().await,
                    Err(error) => Err(error),
                }
            } else {
                generate_text(Arc::clone(&model))
                    .messages(history)
                    .tools(tools)
                    .tools_context(contexts)
                    .await
            };
            let mut actual = seen.lock().unwrap().clone();
            actual.sort_by_key(|(name, _)| *name);
            if valid {
                assert_eq!(result.unwrap().text(), "done");
                assert_eq!(
                    (actual, model.call_count()),
                    (
                        vec![
                            ("number", Some(json!(42))),
                            ("text", Some(json!("selected")))
                        ],
                        1,
                    )
                );
            } else {
                assert!(matches!(
                    result,
                    Err(ferrin_core::Error::InvalidArgument { .. })
                ));
                assert_eq!((actual, model.call_count()), (vec![], 0));
            }
        }
    }
}
