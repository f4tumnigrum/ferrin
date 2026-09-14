//! The generation loop over a mock model: a single step, long message
//! histories, a two-step tool loop and a middleware-wrapped model. The mock
//! answers instantly, so the numbers are the loop's own overhead: prompt
//! conversion, step bookkeeping, tool execution and result assembly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_core::LanguageModelMiddleware;
use ferrin_core::generate_text;
use ferrin_core::middleware::builtin::extract_reasoning;
use ferrin_core::step_count;
use ferrin_core::wrap_language_model;
use ferrin_message::Message;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderError;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::language_model::GenerateResult;
use ferrin_testing::MockLanguageModel;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use serde_json::json;
use tokio::runtime::Runtime;

/// History lengths (messages) for the history benchmark.
const HISTORY_TURNS: [usize; 2] = [10, 50];

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn text_result(text: &str) -> GenerateResult {
    let mut result = GenerateResult::new(vec![Content::text(text)], FinishReason::stop());
    result.usage = Usage::totals(10, 5);
    result
}

fn tool_call_result() -> GenerateResult {
    let mut result = GenerateResult::new(
        vec![Content::ToolCall(ToolCall::new(
            "call-1",
            "get_weather",
            json!({ "city": "Tokyo" }).to_string(),
        ))],
        FinishReason::tool_calls(),
    );
    result.usage = Usage::totals(20, 8);
    result
}

fn text_model(text: &str) -> Arc<MockLanguageModel> {
    MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
        .generate_repeat(text_result(text))
        .build_shared()
}

/// Answers a tool call to every odd call and the final text to every even
/// call, so each `generate_text` run takes exactly two steps.
fn tool_loop_model() -> Arc<MockLanguageModel> {
    let calls = Arc::new(AtomicUsize::new(0));
    MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
        .generate_with(move |_options| {
            let step = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                Ok::<_, ProviderError>(if step.is_multiple_of(2) {
                    tool_call_result()
                } else {
                    text_result("Tokyo is 21 °C and clear.")
                })
            }
        })
        .build_shared()
}

fn weather_tools() -> ToolSet {
    let tool = Tool::function_with_schema(Schema::from_json_schema(json!({
        "type": "object",
        "properties": { "city": { "type": "string" } },
        "required": ["city"],
        "additionalProperties": false
    })))
    .description("Get the weather for a city.")
    .execute(|input: JsonValue, _ctx: ToolContext| async move {
        let city = input["city"].as_str().unwrap_or_default().to_owned();
        Ok::<_, ToolError>(json!({ "city": city, "temperature": 21 }))
    })
    .build();
    ToolSet::new().insert("get_weather", tool).unwrap()
}

/// `turns` alternating user/assistant messages ending with a user turn.
fn history(turns: usize) -> Vec<Message> {
    (0..turns)
        .map(|turn| {
            if turn % 2 == 0 {
                Message::user(format!(
                    "Question number {turn} about backpressure in streams."
                ))
            } else {
                Message::assistant(format!(
                    "Answer number {turn}: backpressure lets consumers slow producers down."
                ))
            }
        })
        .chain(std::iter::once(Message::user("Summarise the discussion.")))
        .collect()
}

fn bench_single_step(c: &mut Criterion) {
    let runtime = runtime();
    let model = text_model("Backpressure lets a consumer slow down a producer.");
    c.bench_function("generate_text/single_step_prompt", |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&model);
            async move {
                black_box(
                    generate_text(model)
                        .prompt("Explain backpressure in two sentences.")
                        .await
                        .unwrap()
                        .text()
                        .len(),
                )
            }
        });
    });
}

fn bench_history(c: &mut Criterion) {
    let runtime = runtime();
    let model = text_model("The discussion covered backpressure.");
    let mut group = c.benchmark_group("generate_text/history");
    for turns in HISTORY_TURNS {
        let history = history(turns);
        group.throughput(Throughput::Elements(u64::try_from(history.len()).unwrap()));
        group.bench_with_input(
            BenchmarkId::from_parameter(history.len()),
            &history,
            |b, history| {
                b.to_async(&runtime).iter(|| {
                    let model = Arc::clone(&model);
                    let history = history.clone();
                    async move {
                        black_box(
                            generate_text(model)
                                .messages(history)
                                .await
                                .unwrap()
                                .text()
                                .len(),
                        )
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_tool_loop(c: &mut Criterion) {
    let runtime = runtime();
    let model = tool_loop_model();
    let tools = weather_tools();
    c.bench_function("generate_text/tool_loop_two_steps", |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&model);
            let tools = tools.clone();
            async move {
                black_box(
                    generate_text(model)
                        .prompt("Weather in Tokyo?")
                        .tools(tools)
                        .stop_when(step_count(3))
                        .await
                        .unwrap()
                        .text()
                        .len(),
                )
            }
        });
    });
}

fn bench_middleware(c: &mut Criterion) {
    let runtime = runtime();
    let inner = text_model(
        "<think>Consider how data flows between producer and consumer.</think>\
         Backpressure lets a consumer slow down a producer.",
    );
    let wrapped = wrap_language_model(
        inner as Arc<dyn DynLanguageModel>,
        [Arc::new(extract_reasoning("think")) as Arc<dyn LanguageModelMiddleware>],
    );
    c.bench_function("generate_text/extract_reasoning_middleware", |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&wrapped);
            async move {
                black_box(
                    generate_text(model)
                        .prompt("Explain backpressure in two sentences.")
                        .await
                        .unwrap()
                        .text()
                        .len(),
                )
            }
        });
    });
}

criterion_group!(
    benches,
    bench_single_step,
    bench_history,
    bench_tool_loop,
    bench_middleware
);
criterion_main!(benches);
