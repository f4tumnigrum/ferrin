use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Agent;
use ferrin_core::AgentCall;
use ferrin_core::StepResult;
use ferrin_core::ToolLoopAgent;
use ferrin_core::agent::PrepareCallInput;
use ferrin_core::generate_text;
use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::Include;
use ferrin_core::generate_text::ParsedToolCall;
use ferrin_core::generate_text::PrepareStepContext;
use ferrin_core::generate_text::StepOverrides;
use ferrin_core::generate_text::approval_policy;
use ferrin_core::step_count;
use ferrin_core::stream_text;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::telemetry::StepEndEvent;
use ferrin_core::telemetry::StepStartEvent;
use ferrin_core::telemetry::Telemetry;
use ferrin_core::telemetry::TelemetryOptions;
use ferrin_core::telemetry::ToolExecutionStartEvent;
use ferrin_message::Message;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_testing::text_parts;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;

fn call_parts(id: &str) -> Vec<StreamPart> {
    vec![
        StreamPart::stream_start(),
        StreamPart::ToolCall(ToolCall::new(id, "inspect", "{}")),
        StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
    ]
}

#[tokio::test]
async fn state_persists_after_compression_in_both_loops() {
    for streaming in [false, true] {
        let model = mock()
            .generate(tool_call_result("one", "inspect", &json!({})))
            .generate(tool_call_result("two", "inspect", &json!({})))
            .generate(text_result("done"))
            .stream(call_parts("one"))
            .stream(call_parts("two"))
            .stream(text_parts(["done"], Usage::default()))
            .build_shared();
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let executed = Arc::clone(&contexts);
        let tools = ToolSet::new()
            .insert(
                "inspect",
                Tool::function_with_schema(Schema::empty_object())
                    .context_schema(Schema::from_json_schema(json!({"type":"string"})))
                    .execute(move |_: JsonValue, context: ferrin_tool::ToolContext| {
                        executed.lock().unwrap().push(context.tools_context);
                        async { Ok::<_, ferrin_tool::ToolError>(json!("ok")) }
                    })
                    .build(),
            )
            .unwrap();
        let prepare = |ctx: &PrepareStepContext<'_>| match ctx.step_number {
            0 => {
                assert_eq!(
                    (
                        ctx.instructions.unwrap().as_messages()[0].content.as_str(),
                        ctx.runtime_context,
                        ctx.tools_context
                    ),
                    (
                        "initial instructions",
                        Some(&json!("initial runtime")),
                        Some(&json!({"inspect":"initial tools"}))
                    )
                );
                StepOverrides::none()
            }
            1 => {
                assert_eq!((ctx.messages.len(), ctx.response_messages.len()), (3, 2));
                StepOverrides::none()
                    .with_messages([Message::user("summary")])
                    .with_instructions("retained instructions")
                    .with_tools_context(json!({"inspect":"retained tools"}))
                    .with_runtime_context(json!({"phase":"second"}))
            }
            2 => {
                assert_eq!(
                    (
                        ctx.messages.len(),
                        ctx.initial_messages.len(),
                        ctx.response_messages.len()
                    ),
                    (3, 1, 4)
                );
                assert_eq!(&ctx.messages[0], &Message::user("summary"));
                assert_eq!(
                    (
                        ctx.instructions.unwrap().as_messages()[0].content.as_str(),
                        ctx.runtime_context,
                        ctx.tools_context
                    ),
                    (
                        "retained instructions",
                        Some(&json!({"phase":"second"})),
                        Some(&json!({"inspect":"retained tools"}))
                    )
                );
                StepOverrides::none()
            }
            _ => panic!("unexpected step"),
        };
        let approvals = Arc::new(Mutex::new(Vec::new()));
        let approval_states = Arc::clone(&approvals);
        let policy = approval_policy(move |_: &ParsedToolCall, context: &ApprovalContext<'_>| {
            approval_states.lock().unwrap().push((
                context.runtime_context.cloned(),
                context.tools_context.cloned(),
            ));
            None
        });
        let hooked = Arc::new(Mutex::new(Vec::new()));
        let hook_states = Arc::clone(&hooked);
        let on_step = move |event: Arc<StepStartEvent>| {
            hook_states
                .lock()
                .unwrap()
                .push(event.runtime_context.clone());
            async {}
        };
        let tool_hooked = Arc::new(Mutex::new(Vec::new()));
        let tool_hook_states = Arc::clone(&tool_hooked);
        let on_tool = move |event: Arc<ToolExecutionStartEvent>| {
            tool_hook_states
                .lock()
                .unwrap()
                .push(event.runtime_context.clone());
            async {}
        };
        let result = if streaming {
            stream_text(Arc::clone(&model))
                .prompt("initial messages")
                .system("initial instructions")
                .tools(tools)
                .tools_context(json!({"inspect":"initial tools"}))
                .runtime_context(json!("initial runtime"))
                .stop_when(step_count(4))
                .prepare_step(prepare)
                .tool_approval(policy)
                .include(Include::all())
                .on_step_start(on_step)
                .on_tool_execution_start(on_tool)
                .await
                .unwrap()
                .consume()
                .await
                .unwrap()
        } else {
            generate_text(Arc::clone(&model))
                .prompt("initial messages")
                .system("initial instructions")
                .tools(tools)
                .tools_context(json!({"inspect":"initial tools"}))
                .runtime_context(json!("initial runtime"))
                .stop_when(step_count(4))
                .prepare_step(prepare)
                .tool_approval(policy)
                .include(Include::all())
                .on_step_start(on_step)
                .on_tool_execution_start(on_tool)
                .await
                .unwrap()
        };
        let runtime = [
            Some(json!("initial runtime")),
            Some(json!({"phase":"second"})),
            Some(json!({"phase":"second"})),
        ];
        assert_eq!(*hooked.lock().unwrap(), runtime);
        assert_eq!(*tool_hooked.lock().unwrap(), runtime[..2]);
        assert_eq!(
            *contexts.lock().unwrap(),
            [Some(json!("initial tools")), Some(json!("retained tools"))]
        );
        assert_eq!(
            *approvals.lock().unwrap(),
            [
                (runtime[0].clone(), Some(json!({"inspect":"initial tools"}))),
                (
                    runtime[1].clone(),
                    Some(json!({"inspect":"retained tools"}))
                ),
            ]
        );
        assert_eq!(
            result
                .steps
                .iter()
                .map(|step| step.runtime_context.clone())
                .collect::<Vec<_>>(),
            runtime
        );
        assert_eq!(result.response_messages().len(), 5);
        let final_messages = result.last_step().request.messages.as_ref().unwrap();
        assert_eq!(final_messages.len(), 3);
        assert_eq!(&final_messages[0], &Message::user("summary"));
        let calls = if streaming {
            model.stream_calls()
        } else {
            model.generate_calls()
        };
        assert_eq!(
            calls[2].prompt[0],
            ferrin_spec::PromptMessage::system("retained instructions")
        );
        let wire = serde_json::to_value(calls[2].to_recordable()).unwrap();
        assert!(!wire.to_string().contains("second"));
    }
}

#[tokio::test]
async fn agent_call_runtime_is_independent_and_can_be_explicitly_replaced_with_null() {
    let agent = ToolLoopAgent::builder(mock().generate_repeat(text_result("done")).build_shared())
        .runtime_context(json!("default"))
        .call_options::<String>()
        .prepare_call(|input: PrepareCallInput<String>| async move {
            assert_eq!(input.defaults.runtime_context, Some(json!("default")));
            let mut call = input.defaults;
            call.runtime_context = Some(json!(input.options));
            Ok(call)
        })
        .prepare_step(|ctx: &PrepareStepContext<'_>| {
            if ctx.runtime_context == Some(&json!("clear")) {
                StepOverrides::none().with_runtime_context(JsonValue::Null)
            } else {
                StepOverrides::none()
            }
        })
        .build();
    let first = agent.generate(AgentCall::prompt("hi").options("one".to_owned()));
    let second = agent.generate(AgentCall::prompt("hi").options("clear".to_owned()));
    let (first, second) = tokio::join!(first, second);
    assert_eq!(
        (
            first.unwrap().last_step().runtime_context.clone(),
            second.unwrap().last_step().runtime_context.clone()
        ),
        (Some(json!("one")), Some(JsonValue::Null))
    );
}

#[derive(Default)]
struct ContextRecorder(Mutex<Vec<Option<JsonValue>>>);

impl Telemetry for ContextRecorder {
    fn on_start<'a>(&'a self, event: &'a StartEvent) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event.runtime_context.clone());
        })
    }
    fn on_step_start<'a>(&'a self, event: &'a StepStartEvent) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event.runtime_context.clone());
        })
    }
    fn on_step_end<'a>(&'a self, event: &'a StepEndEvent) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().extend([
                event.step.runtime_context.clone(),
                event.step.tools_context.clone(),
            ]);
        })
    }
    fn on_end<'a>(&'a self, event: &'a EndEvent) -> ferrin_spec::BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().push(event.runtime_context.clone());
        })
    }
}

#[tokio::test]
async fn runtime_is_available_to_hooks_but_not_telemetry_and_old_results_deserialize() {
    let recorder = Arc::new(ContextRecorder::default());
    let result = generate_text(mock().generate(text_result("done")).build_shared())
        .prompt("hi")
        .runtime_context(json!("private runtime"))
        .tools_context(json!("private tools"))
        .telemetry(TelemetryOptions::enabled().with_integration(recorder.clone()))
        .on_end(|event: Arc<EndEvent>| async move {
            assert_eq!(event.runtime_context, Some(json!("private runtime")));
        })
        .await
        .unwrap();
    assert_eq!(*recorder.0.lock().unwrap(), vec![None; 5]);
    let mut old = serde_json::to_value(result.last_step()).unwrap();
    old.as_object_mut().unwrap().remove("runtime_context");
    old.as_object_mut().unwrap().remove("tools_context");
    let restored: StepResult = serde_json::from_value(old).unwrap();
    let mut expected = result.last_step().clone();
    expected.runtime_context = None;
    expected.tools_context = None;
    assert_eq!(restored, expected);
}

#[tokio::test]
async fn telemetry_contexts_require_independent_explicit_opt_ins() {
    for include_runtime_context in [false, true] {
        for include_tools_context in [false, true] {
            let recorder = Arc::new(ContextRecorder::default());
            stream_text(
                mock()
                    .stream(text_parts(["done"], Usage::default()))
                    .build_shared(),
            )
            .prompt("hi")
            .runtime_context(json!("runtime"))
            .tools_context(json!("tools"))
            .telemetry(TelemetryOptions {
                include_runtime_context,
                include_tools_context,
                ..TelemetryOptions::enabled().with_integration(recorder.clone())
            })
            .await
            .unwrap()
            .consume()
            .await
            .unwrap();
            let runtime = include_runtime_context.then(|| json!("runtime"));
            assert_eq!(
                *recorder.0.lock().unwrap(),
                vec![
                    runtime.clone(),
                    runtime.clone(),
                    runtime.clone(),
                    include_tools_context.then(|| json!("tools")),
                    runtime,
                ]
            );
        }
    }
}
