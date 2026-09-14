//! The Anthropic Messages adapter end to end against the local fixture server:
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
use ferrin_anthropic::AnthropicMessagesLanguageModel;
use ferrin_anthropic::AnthropicSettings;
use ferrin_anthropic::create_anthropic;
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

const MODEL: &str = "claude-sonnet-4-5";

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// A fixture server plus a model pointing at it.
struct Harness {
    server: FixtureServer,
    model: AnthropicMessagesLanguageModel,
}

impl Harness {
    async fn start() -> Self {
        let server = FixtureServer::start().await.unwrap();
        let settings = AnthropicSettings {
            base_url: Some(server.url().join("v1").unwrap()),
            api_key: Some(SecretString::from("test-key".to_owned())),
            id_generator: Some(Arc::new(SequentialIdGenerator::new("id"))),
            ..AnthropicSettings::default()
        };
        let model = create_anthropic(settings).unwrap().messages(MODEL);
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
    let fixture = fixture("messages", "text-basic");
    c.bench_function("anthropic_messages/generate/text-basic", |b| {
        b.to_async(&runtime).iter(|| {
            harness.remount("/v1/messages", &fixture);
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
    let mut group = c.benchmark_group("anthropic_messages/stream");
    for case in ["text-basic-stream", "reasoning-stream"] {
        let fixture = fixture("messages", case);
        group.bench_function(case, |b| {
            b.to_async(&runtime).iter(|| {
                harness.remount("/v1/messages", &fixture);
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
