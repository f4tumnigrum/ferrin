use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ferrin_core::generate_text;
use ferrin_core::generate_text::PrepareStepContext;
use ferrin_core::generate_text::RepairRequest;
use ferrin_core::generate_text::StepOverrides;
use ferrin_core::generate_text::ToolCallRepair;
use ferrin_core::stream_text;
use ferrin_spec::BoxFuture;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use ferrin_tool::callers::ToolCaller;
use ferrin_tool::callers::ToolCallerDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::tool_call_result;

struct RepairToHidden;
impl ToolCallRepair for RepairToHidden {
    fn repair<'a>(
        &'a self,
        request: RepairRequest<'a>,
    ) -> BoxFuture<'a, Result<Option<ToolCall>, ferrin_core::error::BoxError>> {
        Box::pin(async move {
            assert!(!request.tools.contains("hidden"));
            Ok(Some(ToolCall::new("call", "hidden", "{}")))
        })
    }
}

#[tokio::test]
async fn restricted_tools_cannot_execute_from_model_calls_or_repairs() {
    for restriction in ["callers", "active", "empty", "local", "step", "repair"] {
        let executions = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&executions);
        let hidden = Tool::function_with_schema(Schema::empty_object())
            .execute(move |_: JsonValue, _: ToolContext| {
                let count = Arc::clone(&count);
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, ToolError>(json!("side effect"))
                }
            })
            .build();
        let mut tools = ToolSet::new().insert("hidden", hidden).unwrap();
        tools = tools
            .insert(
                "allowed",
                Tool::function_with_schema(Schema::empty_object()).build(),
            )
            .unwrap();
        if restriction == "local" {
            tools = tools
                .insert(
                    "caller",
                    Tool::function_with_schema(Schema::empty_object())
                        .caller(ToolCallerDefinition::local(|callees| {
                            assert!(callees.contains("hidden"));
                            Tool::function_with_schema(Schema::empty_object()).build()
                        }))
                        .build(),
                )
                .unwrap();
        }
        let name = if restriction == "repair" {
            "missing"
        } else {
            "hidden"
        };
        let model = mock()
            .generate(tool_call_result("call", name, &json!({})))
            .stream(vec![
                StreamPart::stream_start(),
                StreamPart::ToolCall(ToolCall::new("call", name, "{}")),
                StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
            ])
            .build_shared();
        let mut generate = generate_text(Arc::clone(&model))
            .prompt("hi")
            .tools(tools.clone());
        let mut stream = stream_text(Arc::clone(&model)).prompt("hi").tools(tools);
        match restriction {
            "callers" => {
                let callers = [("hidden".into(), Vec::new())].into_iter().collect();
                generate = generate.tool_callers(callers);
                stream = stream.tool_callers([("hidden".into(), Vec::new())].into_iter().collect());
            }
            "local" => {
                let callers = [("hidden".into(), vec![ToolCaller::Tool("caller".into())])]
                    .into_iter()
                    .collect();
                generate = generate.tool_callers(callers);
                stream = stream.tool_callers(
                    [("hidden".into(), vec![ToolCaller::Tool("caller".into())])]
                        .into_iter()
                        .collect(),
                );
            }
            "empty" => {
                generate = generate.active_tools(Vec::<ferrin_spec::ToolName>::new());
                stream = stream.active_tools(Vec::<ferrin_spec::ToolName>::new());
            }
            "step" => {
                generate = generate.prepare_step(|_: &PrepareStepContext<'_>| {
                    StepOverrides::none().with_active_tools(["allowed"])
                });
                stream = stream.prepare_step(|_: &PrepareStepContext<'_>| {
                    StepOverrides::none().with_active_tools(["allowed"])
                });
            }
            _ => {
                generate = generate.active_tools(["allowed"]);
                stream = stream.active_tools(["allowed"]);
            }
        }
        if restriction == "repair" {
            generate = generate.repair_tool_call(RepairToHidden);
            stream = stream.repair_tool_call(RepairToHidden);
        }
        let generated = generate.await.unwrap();
        let streamed = stream.await.unwrap().consume().await.unwrap();
        for result in [generated, streamed] {
            assert!(
                result.last_step().tool_calls().next().unwrap().invalid,
                "{restriction}"
            );
            assert_eq!(result.last_step().tool_results().count(), 0);
        }
        assert_eq!(executions.load(Ordering::SeqCst), 0, "{restriction}");
        for call in model
            .generate_calls()
            .iter()
            .chain(model.stream_calls().iter())
        {
            assert!(
                call.tools
                    .iter()
                    .all(|tool| tool.name().as_str() != "hidden")
            );
        }
    }
}
