//! The OpenAI Responses adapter end to end against the local fixture server:
//! request assembly, HTTP, SSE decoding and stream-part mapping. The
//! server replays recorded fixtures without delay, so the numbers are the
//! adapter and transport overhead per call.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_openai::OpenAiResponsesLanguageModel;
use ferrin_openai::OpenAiSettings;
use ferrin_openai::create_openai;
use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use ferrin_testing::SequentialIdGenerator;
use futures_util::StreamExt;
use http::Method;
use secrecy::SecretString;
use tokio::runtime::Runtime;

const MODEL: &str = "gpt-5";

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// A fixture server plus a model pointing at it.
struct Harness {
    server: FixtureServer,
    model: OpenAiResponsesLanguageModel,
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
        let model = create_openai(settings).unwrap().responses(MODEL);
        Self { server, model }
    }

    /// Mounts `fixture` at `path` and clears the recorded requests, so the
    /// server's memory does not grow with the iteration count.
    fn remount(&self, path: &str, fixture: &Fixture) {
        self.server.reset();
        self.server.mount(Method::POST, path, fixture.clone());
    }
}

fn fixture(area: &str, case: &str) -> Fixture {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(area);
    Fixture::load(dir, case).unwrap()
}

fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text(
        "Explain backpressure in two sentences.",
    )])
}

fn bench_generate(c: &mut Criterion) {
    let runtime = runtime();
    let harness = runtime.block_on(Harness::start());
    let fixture = fixture("responses", "text-basic");
    c.bench_function("openai_responses/generate/text-basic", |b| {
        b.to_async(&runtime).iter(|| {
            harness.remount("/v1/responses", &fixture);
            async {
                black_box(
                    harness
                        .model
                        .do_generate(options())
                        .await
                        .unwrap()
                        .content
                        .len(),
                )
            }
        });
    });
}

fn bench_stream(c: &mut Criterion) {
    let runtime = runtime();
    let harness = runtime.block_on(Harness::start());
    let mut group = c.benchmark_group("openai_responses/stream");
    for case in ["text-basic-stream", "reasoning-stream"] {
        let fixture = fixture("responses", case);
        group.bench_function(case, |b| {
            b.to_async(&runtime).iter(|| {
                harness.remount("/v1/responses", &fixture);
                async {
                    let result = harness.model.do_stream(options()).await.unwrap();
                    black_box(result.stream.count().await)
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_generate, bench_stream);
criterion_main!(benches);
