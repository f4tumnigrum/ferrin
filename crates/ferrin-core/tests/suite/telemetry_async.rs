use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ferrin_core::Error;
use ferrin_core::Telemetry;
use ferrin_core::TelemetryOptions;
use ferrin_core::generate_text;
use ferrin_core::stream_text;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::ModelCallContext;
use ferrin_core::telemetry::ModelCallOutcome;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::telemetry::ToolExecutionContext;
use ferrin_core::telemetry::ToolOutcome;
use ferrin_spec::BoxFuture;
use ferrin_testing::text_parts;
use ferrin_tool::ToolError;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Barrier;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

struct AwaitedIntegration {
    barrier: Arc<Barrier>,
    starts: Arc<AtomicUsize>,
    ends: Arc<AtomicUsize>,
}

impl Telemetry for AwaitedIntegration {
    fn on_start<'a>(&'a self, _event: &'a StartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.barrier.wait().await;
            self.starts.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn on_end<'a>(&'a self, _event: &'a EndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.barrier.wait().await;
            self.ends.fetch_add(1, Ordering::SeqCst);
        })
    }
}

struct PanickingIntegration;

impl Telemetry for PanickingIntegration {
    fn on_start<'a>(&'a self, _event: &'a StartEvent) -> BoxFuture<'a, ()> {
        panic!("integration invocation panic")
    }

    fn on_end<'a>(&'a self, _event: &'a EndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async { panic!("integration polling panic") })
    }
}

#[tokio::test]
async fn telemetry_callbacks_settle_concurrently_and_isolate_panics() {
    for streaming in [false, true] {
        let barrier = Arc::new(Barrier::new(2));
        let starts = Arc::new(AtomicUsize::new(0));
        let ends = Arc::new(AtomicUsize::new(0));
        let mut options =
            TelemetryOptions::enabled().with_integration(Arc::new(PanickingIntegration));
        for _ in 0..2 {
            options = options.with_integration(Arc::new(AwaitedIntegration {
                barrier: Arc::clone(&barrier),
                starts: Arc::clone(&starts),
                ends: Arc::clone(&ends),
            }));
        }
        let model = mock()
            .generate(text_result("done"))
            .stream(text_parts(["done"], ferrin_spec::Usage::default()))
            .build_shared();
        let started = Arc::clone(&starts);
        let start_hook = move |_: Arc<StartEvent>| {
            let started = Arc::clone(&started);
            async move {
                assert_eq!(started.load(Ordering::SeqCst), 2);
            }
        };
        let result = if streaming {
            stream_text(model)
                .prompt("hello")
                .telemetry(options)
                .on_start(start_hook)
                .await
                .unwrap()
                .consume()
                .await
                .unwrap()
        } else {
            generate_text(model)
                .prompt("hello")
                .telemetry(options)
                .on_start(start_hook)
                .await
                .unwrap()
        };
        assert_eq!(
            (result.text(), ends.load(Ordering::SeqCst)),
            ("done".to_owned(), 2)
        );
    }
}

struct OrderedWrapper {
    name: &'static str,
    order: Arc<Mutex<Vec<String>>>,
}

impl Telemetry for OrderedWrapper {
    fn execute_language_model_call<'a>(
        &'a self,
        _ctx: &'a ModelCallContext,
        call: BoxFuture<'a, Result<ModelCallOutcome, Error>>,
    ) -> BoxFuture<'a, Result<ModelCallOutcome, Error>> {
        Box::pin(async move {
            self.order
                .lock()
                .unwrap()
                .push(format!("{}:model:before", self.name));
            let result = call.await;
            self.order
                .lock()
                .unwrap()
                .push(format!("{}:model:after", self.name));
            result
        })
    }

    fn execute_tool<'a>(
        &'a self,
        _ctx: &'a ToolExecutionContext,
        call: BoxFuture<'a, Result<ToolOutcome, ToolError>>,
    ) -> BoxFuture<'a, Result<ToolOutcome, ToolError>> {
        Box::pin(async move {
            self.order
                .lock()
                .unwrap()
                .push(format!("{}:tool:before", self.name));
            let result = call.await;
            self.order
                .lock()
                .unwrap()
                .push(format!("{}:tool:after", self.name));
            result
        })
    }
}

#[tokio::test]
async fn telemetry_last_registered_wrapper_is_outermost() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let options =
        ["first", "last"]
            .into_iter()
            .fold(TelemetryOptions::enabled(), |options, name| {
                options.with_integration(Arc::new(OrderedWrapper {
                    name,
                    order: Arc::clone(&order),
                }))
            });
    generate_text(
        mock()
            .generate(tool_call_result(
                "call",
                "get_weather",
                &json!({"city":"Rome"}),
            ))
            .build_shared(),
    )
    .prompt("hello")
    .tools(weather_tools())
    .telemetry(options)
    .await
    .unwrap();
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "last:model:before",
            "first:model:before",
            "first:model:after",
            "last:model:after",
            "last:tool:before",
            "first:tool:before",
            "first:tool:after",
            "last:tool:after",
        ]
    );
}
