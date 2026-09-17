use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::HookFn;
use ferrin_core::Hooks;
use ferrin_core::StepResult;
use ferrin_core::generate_text;
use ferrin_core::step_count;
use ferrin_core::stream_text;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::ModelCallEndEvent;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::telemetry::ToolExecutionEndEvent;
use ferrin_spec::Usage;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Barrier;
use tokio::sync::Notify;

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

#[tokio::test(start_paused = true)]
async fn hooks_rendezvous_concurrently_and_all_finish_before_emit_returns() {
    let barrier = Arc::new(Barrier::new(2));
    let started = Arc::new(Mutex::new(Vec::new()));
    let completed = Arc::new(Mutex::new(Vec::new()));
    let hooks: Vec<Arc<dyn HookFn<()>>> = (0..2)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            let started = Arc::clone(&started);
            let completed = Arc::clone(&completed);
            Arc::new(move |_: Arc<()>| {
                started.lock().unwrap().push(index);
                let barrier = Arc::clone(&barrier);
                let completed = Arc::clone(&completed);
                async move {
                    barrier.wait().await;
                    completed.lock().unwrap().push(index);
                }
            }) as Arc<dyn HookFn<()>>
        })
        .collect();

    // Paused time makes this a deterministic deadlock guard for serial dispatch.
    tokio::time::timeout(Duration::from_secs(1), Hooks::emit(&hooks, Arc::new(())))
        .await
        .expect("both callbacks must reach the barrier");

    let mut completed = completed.lock().unwrap().clone();
    completed.sort_unstable();
    assert_eq!(
        (started.lock().unwrap().clone(), completed),
        (vec![0, 1], vec![0, 1])
    );
}

fn panicking_hooks<E: 'static>(log: &Arc<Mutex<Vec<&'static str>>>) -> Vec<Arc<dyn HookFn<E>>> {
    let rendezvous = Arc::new(Notify::new());
    vec![
        Arc::new({
            let log = Arc::clone(log);
            move |_: Arc<E>| -> std::future::Ready<()> {
                log.lock().unwrap().push("sync-panic");
                panic!("synchronous hook panic");
            }
        }),
        Arc::new({
            let log = Arc::clone(log);
            let rendezvous = Arc::clone(&rendezvous);
            move |_: Arc<E>| {
                log.lock().unwrap().push("async-start");
                let log = Arc::clone(&log);
                let rendezvous = Arc::clone(&rendezvous);
                async move {
                    rendezvous.notified().await;
                    log.lock().unwrap().push("async-panic");
                    panic!("asynchronous hook panic");
                }
            }
        }),
        Arc::new({
            let log = Arc::clone(log);
            move |_: Arc<E>| {
                log.lock().unwrap().push("survivor-start");
                let log = Arc::clone(&log);
                let rendezvous = Arc::clone(&rendezvous);
                async move {
                    rendezvous.notify_one();
                    log.lock().unwrap().push("survivor-end");
                }
            }
        }),
    ]
}

#[tokio::test(start_paused = true)]
async fn generation_isolates_synchronous_and_asynchronous_hook_panics() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let hooks = Hooks {
        on_start: panicking_hooks(&log),
        ..Hooks::default()
    };
    let model = mock().generate(text_result("done")).build_shared();
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        generate_text(model).prompt("hi").hooks(hooks),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(
        (result.text(), log.lock().unwrap().clone()),
        (
            "done".to_owned(),
            vec![
                "sync-panic",
                "async-start",
                "survivor-start",
                "survivor-end",
                "async-panic",
            ],
        )
    );
}

#[tokio::test(start_paused = true)]
async fn streaming_isolates_synchronous_and_asynchronous_chunk_hook_panics() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let hooks = Hooks {
        on_chunk: panicking_hooks(&log),
        ..Hooks::default()
    };
    let model = mock()
        .stream(text_parts(["done"], Usage::totals(1, 1)))
        .build_shared();
    let (events, result) = tokio::time::timeout(Duration::from_secs(1), async {
        let stream = stream_text(model).prompt("hi").hooks(hooks).await.unwrap();
        let (events, completion) = stream.split();
        let events: Vec<_> = events.collect().await;
        (events, completion.await.unwrap())
    })
    .await
    .unwrap();

    let expected = [
        "sync-panic",
        "async-start",
        "survivor-start",
        "survivor-end",
        "async-panic",
    ]
    .repeat(events.len());
    assert_eq!(
        (result.text(), log.lock().unwrap().clone()),
        ("done".to_owned(), expected)
    );
    assert_eq!(events.last().unwrap().kind_name(), "finish");
}
