//! The whole stack end to end: `ferrin::stream_text` over the OpenAI
//! Responses adapter against a local fixture server that replays a synthetic
//! text stream without delay. Measures the per-stream overhead of request
//! assembly, HTTP, SSE decoding, provider mapping and the core pipeline, and
//! how it scales with concurrent streams.

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
use ferrin::openai::OpenAiProvider;
use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::serde_json::Value;
use ferrin::serde_json::json;
use ferrin::stream_text;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use ferrin_testing::SequentialIdGenerator;
use futures_util::StreamExt;
use http::Method;
use secrecy::SecretString;
use tokio::runtime::Runtime;
use tokio::task::JoinSet;

const PATH: &str = "/v1/responses";
const MODEL: &str = "gpt-5";
/// Delta counts for the single-stream benchmark.
const DELTAS: [usize; 2] = [20, 200];
/// Deltas per stream in the concurrency benchmark.
const CONCURRENT_DELTAS: usize = 50;
/// Concurrent streams per iteration.
const CONCURRENCY: [usize; 3] = [1, 16, 64];

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// A Responses API text stream with `deltas` output-text deltas, shaped like
/// the recorded `text-basic-stream` fixture.
fn responses_stream(deltas: usize) -> Fixture {
    let mut events = vec![
        json!({"type": "response.created", "sequence_number": 0, "response": {
            "id": "resp_bench", "object": "response", "created_at": 1_757_721_700,
            "model": MODEL, "status": "in_progress", "output": []}}),
        json!({"type": "response.output_item.added", "sequence_number": 1, "output_index": 0,
            "item": {"type": "message", "id": "msg_1", "status": "in_progress", "role": "assistant", "content": []}}),
        json!({"type": "response.content_part.added", "sequence_number": 2, "item_id": "msg_1",
            "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}}),
    ];
    let mut text = String::new();
    for index in 0..deltas {
        let delta = format!("token{index} ");
        text.push_str(&delta);
        events.push(
            json!({"type": "response.output_text.delta", "sequence_number": events.len(),
            "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": delta}),
        );
    }
    let part: Value = json!({"type": "output_text", "text": text, "annotations": []});
    let item: Value = json!({"type": "message", "id": "msg_1", "status": "completed", "role": "assistant", "content": [part]});
    events.push(
        json!({"type": "response.output_text.done", "sequence_number": events.len(),
        "item_id": "msg_1", "output_index": 0, "content_index": 0, "text": text}),
    );
    events.push(
        json!({"type": "response.content_part.done", "sequence_number": events.len(),
        "item_id": "msg_1", "output_index": 0, "content_index": 0, "part": part}),
    );
    events.push(
        json!({"type": "response.output_item.done", "sequence_number": events.len(),
        "output_index": 0, "item": item}),
    );
    events.push(
        json!({"type": "response.completed", "sequence_number": events.len(), "response": {
        "id": "resp_bench", "object": "response", "created_at": 1_757_721_700, "model": MODEL,
        "status": "completed", "output": [item], "service_tier": "default",
        "usage": {"input_tokens": 12, "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": deltas, "output_tokens_details": {"reasoning_tokens": 0}}}}),
    );
    Fixture::sse_json(&events)
}

/// A fixture server plus a provider pointing at it.
struct Harness {
    server: FixtureServer,
    openai: Arc<OpenAiProvider>,
}

impl Harness {
    async fn start() -> Self {
        let server = FixtureServer::start().await.unwrap();
        let settings = OpenAiSettings {
            base_url: Some(server.url().join("v1").unwrap()),
            api_key: Some(SecretString::from("test-key".to_owned())),
            id_generator: Some(Arc::new(SequentialIdGenerator::new("id"))),
            ..OpenAiSettings::default()
        };
        Self {
            server,
            openai: Arc::new(create_openai(settings).unwrap()),
        }
    }

    /// Mounts `fixture` and clears the recorded requests, so the server's
    /// memory does not grow with the iteration count.
    fn remount(&self, fixture: &Fixture) {
        self.server.reset();
        self.server.mount(Method::POST, PATH, fixture.clone());
    }
}

/// Runs one stream and returns the number of text deltas received.
async fn one_stream(openai: &OpenAiProvider) -> usize {
    stream_text(openai.responses(MODEL))
        .prompt("Explain backpressure in two sentences.")
        .await
        .unwrap()
        .text_stream()
        .count()
        .await
}

fn bench_stream_text(c: &mut Criterion) {
    let runtime = runtime();
    let harness = runtime.block_on(Harness::start());
    let mut group = c.benchmark_group("end_to_end/stream_text");
    for deltas in DELTAS {
        let fixture = responses_stream(deltas);
        group.throughput(Throughput::Elements(u64::try_from(deltas).unwrap()));
        group.bench_function(BenchmarkId::from_parameter(deltas), |b| {
            b.to_async(&runtime).iter(|| {
                harness.remount(&fixture);
                async { black_box(one_stream(&harness.openai).await) }
            });
        });
    }
    group.finish();
}

fn bench_concurrent_streams(c: &mut Criterion) {
    let runtime = runtime();
    let harness = runtime.block_on(Harness::start());
    let fixture = responses_stream(CONCURRENT_DELTAS);
    let mut group = c.benchmark_group("end_to_end/concurrent_streams");
    for concurrency in CONCURRENCY {
        group.throughput(Throughput::Elements(u64::try_from(concurrency).unwrap()));
        group.bench_function(BenchmarkId::from_parameter(concurrency), |b| {
            b.to_async(&runtime).iter(|| {
                harness.remount(&fixture);
                let openai = Arc::clone(&harness.openai);
                async move {
                    let mut tasks = JoinSet::new();
                    for _ in 0..concurrency {
                        let openai = Arc::clone(&openai);
                        tasks.spawn(async move { one_stream(&openai).await });
                    }
                    let mut total = 0;
                    while let Some(count) = tasks.join_next().await {
                        total += count.unwrap();
                    }
                    black_box(total)
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_stream_text, bench_concurrent_streams);
criterion_main!(benches);
