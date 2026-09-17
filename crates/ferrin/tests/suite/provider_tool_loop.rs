//! Factory caller bindings and deferred outcomes through the public generation API.

use std::sync::Arc;

use ferrin::prelude::*;
use ferrin::spec::ToolCall;
use ferrin::tool::ToolCaller;
use ferrin::tool::ToolCallers;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn provider_factories_drive_deferred_steps_and_keep_callee_routing() {
    let openai = ferrin::openai::create_openai(ferrin::openai::OpenAiSettings::default()).unwrap();
    let anthropic =
        ferrin::anthropic::create_anthropic(ferrin::anthropic::AnthropicSettings::default())
            .unwrap();
    let factories = [
        (
            openai.tools().programmatic_tool_calling(),
            "openai",
            "programmatic",
            "allowedCallers",
        ),
        (
            anthropic.tools().code_execution_20250825(),
            "anthropic",
            "code_execution_20250825",
            "allowedCallers",
        ),
    ];
    for (caller, provider, caller_name, caller_key) in factories {
        for streaming in [false, true] {
            let mut program = ToolCall::new(
                "program-call",
                "program",
                r#"{"code":"inspect()","fingerprint":"f"}"#,
            );
            program.provider_executed = true;
            let metadata: ProviderOptions = [(
                provider.to_owned(),
                [
                    (
                        "caller".to_owned(),
                        json!({"type":caller_name,"toolCallId":"program-call"}),
                    ),
                    (
                        "parallelToolCall".to_owned(),
                        json!({"id":"parallel-parent","index":0}),
                    ),
                ]
                .into_iter()
                .collect(),
            )]
            .into_iter()
            .collect();
            let mut inspect = ToolCall::new("inspect-call", "inspect", "{}");
            inspect.provider_metadata = Some(metadata.clone());
            let first = vec![
                Content::ToolCall(program.clone()),
                Content::ToolCall(inspect.clone()),
            ];
            let final_result = ferrin::spec::language_model::content::ProviderToolResult {
                tool_call_id: "program-call".into(),
                tool_name: "program".into(),
                result: json!({"result":"done","status":"completed"}),
                is_error: false,
                preliminary: false,
                dynamic: false,
                provider_metadata: None,
            };
            let second = vec![
                Content::ToolResult(final_result.clone()),
                Content::text("done"),
            ];
            let model = MockLanguageModel::builder()
                .generate(GenerateResult::new(first, FinishReason::tool_calls()))
                .generate(GenerateResult::new(
                    vec![Content::text("working")],
                    FinishReason::stop(),
                ))
                .generate(GenerateResult::new(second, FinishReason::stop()))
                .stream(vec![
                    StreamPart::stream_start(),
                    StreamPart::ToolCall(program),
                    StreamPart::ToolCall(inspect),
                    StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
                ])
                .stream(ferrin_testing::text_parts(["working"], Usage::default()))
                .stream(vec![
                    StreamPart::stream_start(),
                    StreamPart::ToolResult(final_result),
                    StreamPart::TextStart {
                        id: "text".into(),
                        provider_metadata: None,
                    },
                    StreamPart::text_delta("text", "done"),
                    StreamPart::TextEnd {
                        id: "text".into(),
                        provider_metadata: None,
                    },
                    StreamPart::finish(FinishReason::stop(), Usage::default()),
                ])
                .build_shared();
            let tools = ToolSet::new()
                .insert("program", caller.clone())
                .unwrap()
                .insert(
                    "inspect",
                    Tool::function_with_schema(Schema::empty_object())
                        .execute(|input: JsonValue, _| async move { Ok::<_, ToolError>(input) })
                        .build(),
                )
                .unwrap();
            let callers: ToolCallers =
                [("inspect".into(), vec![ToolCaller::Tool("program".into())])]
                    .into_iter()
                    .collect();
            let result = if streaming {
                stream_text(Arc::clone(&model))
                    .prompt("run")
                    .tools(tools)
                    .tool_callers(callers)
                    .stop_when(step_count(3))
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                generate_text(Arc::clone(&model))
                    .prompt("run")
                    .tools(tools)
                    .tool_callers(callers)
                    .stop_when(step_count(3))
                    .await
                    .unwrap()
            };
            assert_eq!((result.steps.len(), result.text()), (3, "done".into()));
            let result_metadata = result.steps[0]
                .tool_results()
                .next()
                .unwrap()
                .provider_metadata
                .clone();
            assert_eq!(result_metadata, Some(metadata.clone()));
            let calls = if streaming {
                model.stream_calls()
            } else {
                model.generate_calls()
            };
            let definition = calls[0]
                .tools
                .iter()
                .find(|tool| {
                    matches!(tool,
                ferrin::spec::ToolDefinition::Function {name, ..} if name.as_str() == "inspect")
                })
                .unwrap();
            let ferrin::spec::ToolDefinition::Function {
                provider_options: Some(options),
                ..
            } = definition
            else {
                panic!("callee must carry provider caller binding");
            };
            assert_eq!(options[provider][caller_key], json!([caller_name]));
            let ferrin::spec::PromptMessage::Tool { content, .. } = calls[1].prompt.last().unwrap()
            else {
                panic!("next step must contain callee result");
            };
            let ferrin::spec::language_model::prompt::ToolPromptPart::ToolResult(tool_result) =
                &content[0]
            else {
                panic!("expected local tool result");
            };
            assert_eq!(tool_result.provider_options, Some(metadata.clone()));
        }
    }
}
