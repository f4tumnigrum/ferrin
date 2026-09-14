use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::StepResult;
use ferrin_core::generate_text;
use ferrin_core::step_count;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::ModelCallEndEvent;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::telemetry::ToolExecutionEndEvent;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

#[tokio::test]
async fn hooks_fire_in_lifecycle_order() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let push = |log: &Arc<Mutex<Vec<String>>>, entry: String| {
        log.lock().unwrap().push(entry);
    };
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Rome" }),
        ))
        .generate(text_result("done"))
        .build_shared();

    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(weather_tools())
        .stop_when(step_count(5))
        .on_start({
            let log = Arc::clone(&log);
            move |event: Arc<StartEvent>| {
                push(&log, format!("start:{}", event.model.model_id));
                async {}
            }
        })
        .on_language_model_call_end({
            let log = Arc::clone(&log);
            move |event: Arc<ModelCallEndEvent>| {
                push(&log, format!("model-end:{}", event.step_number));
                async {}
            }
        })
        .on_tool_execution_end({
            let log = Arc::clone(&log);
            move |event: Arc<ToolExecutionEndEvent>| {
                push(&log, format!("tool-end:{}", event.tool_name));
                async {}
            }
        })
        .on_step_end({
            let log = Arc::clone(&log);
            move |step: Arc<StepResult>| {
                push(&log, format!("step-end:{}", step.step_number));
                async {}
            }
        })
        .on_end({
            let log = Arc::clone(&log);
            move |event: Arc<EndEvent>| {
                push(&log, format!("end:{}", event.steps.len()));
                async {}
            }
        })
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 2);
    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "start:mock-model",
            "model-end:0",
            "tool-end:get_weather",
            "step-end:0",
            "model-end:1",
            "step-end:1",
            "end:2",
        ]
    );
}
