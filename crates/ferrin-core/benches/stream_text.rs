//! The streaming pipeline over a mock model: text deltas through
//! `text_stream`, the raw event stream, full consumption into a result, and
//! the smooth-stream transform re-chunking partial words.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;
use std::sync::Arc;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_core::stream_text;
use ferrin_core::stream_text::smooth_stream;
use ferrin_core::stream_text::transforms::SmoothStreamConfig;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_testing::MockLanguageModel;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use tokio::runtime::Runtime;

/// Delta counts for the text-stream benchmark.
const DELTAS: [usize; 2] = [100, 1_000];
/// Delta count for the remaining benchmarks.
const LONG: usize = 1_000;

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Whole-word deltas.
fn words(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("token{index} ")).collect()
}

/// Partial-word deltas that the smooth-stream transform has to re-chunk.
fn fragments(count: usize) -> Vec<String> {
    const PIECES: [&str; 4] = ["hel", "lo ", "wor", "ld "];
    (0..count)
        .map(|index| PIECES[index % 4].to_owned())
        .collect()
}

fn streaming_model(parts: Vec<StreamPart>) -> Arc<MockLanguageModel> {
    MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
        .stream_repeat(parts)
        .build_shared()
}

fn elements(count: usize) -> Throughput {
    Throughput::Elements(u64::try_from(count).unwrap())
}

fn bench_text_stream(c: &mut Criterion) {
    let runtime = runtime();
    let mut group = c.benchmark_group("stream_text/text_stream");
    for count in DELTAS {
        let model = streaming_model(text_parts(words(count), Usage::totals(10, 500)));
        group.throughput(elements(count));
        group.bench_with_input(BenchmarkId::from_parameter(count), &model, |b, model| {
            b.to_async(&runtime).iter(|| {
                let model = Arc::clone(model);
                async move {
                    let result = stream_text(model).prompt("hi").await.unwrap();
                    black_box(result.text_stream().count().await)
                }
            });
        });
    }
    group.finish();
}

fn bench_events_and_consume(c: &mut Criterion) {
    let runtime = runtime();
    let model = streaming_model(text_parts(words(LONG), Usage::totals(10, 500)));
    let mut group = c.benchmark_group("stream_text");
    group.throughput(elements(LONG));
    group.bench_function(BenchmarkId::new("events", LONG), |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&model);
            async move {
                let result = stream_text(model).prompt("hi").await.unwrap();
                let (events, _completion) = result.split();
                black_box(events.count().await)
            }
        });
    });
    group.bench_function(BenchmarkId::new("consume", LONG), |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&model);
            async move {
                let result = stream_text(model).prompt("hi").await.unwrap();
                black_box(result.consume().await.unwrap().text().len())
            }
        });
    });
    group.finish();
}

fn bench_smooth_stream(c: &mut Criterion) {
    let runtime = runtime();
    let model = streaming_model(text_parts(fragments(LONG), Usage::totals(10, 500)));
    let mut group = c.benchmark_group("stream_text/smooth_stream");
    group.throughput(elements(LONG));
    group.bench_function(BenchmarkId::new("word", LONG), |b| {
        b.to_async(&runtime).iter(|| {
            let model = Arc::clone(&model);
            async move {
                let result = stream_text(model)
                    .prompt("hi")
                    .transform(smooth_stream(SmoothStreamConfig::new().delay(None)))
                    .await
                    .unwrap();
                black_box(result.text_stream().count().await)
            }
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_text_stream,
    bench_events_and_consume,
    bench_smooth_stream
);
criterion_main!(benches);
