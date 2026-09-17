use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::StepResult;
use ferrin_core::generate_text;
use ferrin_core::generate_text::PrepareStepContext;
use ferrin_core::generate_text::StepOverrides;
use ferrin_core::generate_text::StopCondition;
use ferrin_core::step_count;
use ferrin_core::stream_text;
use ferrin_spec::BoxFuture;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_tool::DescriptionContext;
use ferrin_tool::LocalProcessSandbox;
use ferrin_tool::Sandbox;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Barrier;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

fn call_parts(id: &str) -> Vec<StreamPart> {
    vec![
        StreamPart::stream_start(),
        StreamPart::ToolCall(ToolCall::new(id, "inspect", "{}")),
        StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
    ]
}

#[tokio::test]
async fn initial_instructions_and_step_sandbox_match_in_both_loops() {
    for streaming in [false, true] {
        let model = mock()
            .generate(tool_call_result("one", "inspect", &json!({})))
            .generate(tool_call_result("two", "inspect", &json!({})))
            .generate(text_result("done"))
            .stream(call_parts("one"))
            .stream(call_parts("two"))
            .stream(ferrin_testing::text_parts(["done"], Usage::default()))
            .build_shared();
        let base: Arc<dyn Sandbox> =
            Arc::new(LocalProcessSandbox::new(".").with_description("base sandbox"));
        let override_sandbox: Arc<dyn Sandbox> =
            Arc::new(LocalProcessSandbox::new(".").with_description("step sandbox"));
        let descriptions = Arc::new(Mutex::new(Vec::new()));
        let description_events = Arc::clone(&descriptions);
        let tools = ToolSet::new()
            .insert(
                "inspect",
                Tool::function_with_schema(Schema::empty_object())
                    .description_fn(move |ctx: DescriptionContext| {
                        let description = ctx.sandbox.as_ref().unwrap().description().to_owned();
                        description_events.lock().unwrap().push(description.clone());
                        async move { description }
                    })
                    .execute(|_: JsonValue, ctx: ToolContext| async move {
                        Ok::<_, ToolError>(json!(ctx.sandbox.as_ref().unwrap().description()))
                    })
                    .build(),
            )
            .unwrap();
        let prepare = move |ctx: &PrepareStepContext<'_>| {
            assert_eq!(
                (
                    ctx.initial_instructions.unwrap().as_messages()[0]
                        .content
                        .as_str(),
                    ctx.instructions.unwrap().as_messages()[0].content.as_str(),
                    ctx.model.model_id().as_str(),
                    ctx.sandbox.unwrap().description(),
                ),
                (
                    "initial",
                    if ctx.step_number == 0 {
                        "initial"
                    } else {
                        "retained"
                    },
                    "mock-model",
                    "base sandbox",
                ),
            );
            if ctx.step_number == 0 {
                StepOverrides::none()
                    .with_model(Arc::clone(ctx.model))
                    .with_instructions("retained")
                    .with_sandbox(Arc::clone(&override_sandbox))
            } else {
                StepOverrides::none()
            }
        };
        let result = if streaming {
            stream_text(Arc::clone(&model))
                .prompt("inspect")
                .system("initial")
                .tools(tools)
                .sandbox(base)
                .prepare_step(prepare)
                .stop_when(step_count(3))
                .await
                .unwrap()
                .consume()
                .await
                .unwrap()
        } else {
            generate_text(Arc::clone(&model))
                .prompt("inspect")
                .system("initial")
                .tools(tools)
                .sandbox(base)
                .prepare_step(prepare)
                .stop_when(step_count(3))
                .await
                .unwrap()
        };
        assert_eq!(
            result
                .steps
                .iter()
                .flat_map(StepResult::tool_results)
                .map(|result| result.output.clone())
                .collect::<Vec<_>>(),
            vec![json!("step sandbox"), json!("base sandbox")],
        );
        assert_eq!(
            *descriptions.lock().unwrap(),
            vec!["step sandbox", "base sandbox", "base sandbox"],
        );
    }
}

struct CoordinatedStop {
    barrier: Arc<Barrier>,
    stop: bool,
}

impl StopCondition for CoordinatedStop {
    fn should_stop<'a>(&'a self, _steps: &'a [StepResult]) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.barrier.wait().await;
            self.stop
        })
    }
}

#[tokio::test]
async fn stop_predicates_are_awaited_concurrently() {
    for streaming in [false, true] {
        let barrier = Arc::new(Barrier::new(2));
        let first = CoordinatedStop {
            barrier: Arc::clone(&barrier),
            stop: false,
        };
        let second = CoordinatedStop {
            barrier,
            stop: true,
        };
        let call = tool_call_result("one", "get_weather", &json!({"city":"Bern"}));
        let model = mock()
            .generate(call)
            .stream(vec![
                StreamPart::stream_start(),
                StreamPart::ToolCall(ToolCall::new("one", "get_weather", r#"{"city":"Bern"}"#)),
                StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
            ])
            .build_shared();
        let run = async {
            if streaming {
                stream_text(model)
                    .prompt("weather")
                    .tools(weather_tools())
                    .stop_when(first)
                    .stop_when(second)
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                generate_text(model)
                    .prompt("weather")
                    .tools(weather_tools())
                    .stop_when(first)
                    .stop_when(second)
                    .await
                    .unwrap()
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap();
        assert_eq!(result.steps.len(), 1);
    }
}

#[tokio::test]
async fn step_count_matches_exact_completed_count() {
    let model = mock()
        .generate(tool_call_result(
            "one",
            "get_weather",
            &json!({"city":"Bern"}),
        ))
        .generate(text_result("done"))
        .build_shared();
    let result = generate_text(model)
        .prompt("weather")
        .tools(weather_tools())
        .stop_when(step_count(0))
        .await
        .unwrap();
    assert_eq!(
        (
            result.steps.len(),
            step_count(0).should_stop(&result.steps).await,
            step_count(1).should_stop(&result.steps).await,
            step_count(2).should_stop(&result.steps).await,
        ),
        (2, false, false, true),
    );
}
