//! SSE decoding: `SseDecoder::feed` over a synthetic body split into chunks
//! of different sizes, and `decode_stream` over an in-memory body stream.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;

use bytes::Bytes;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_provider_util::TransportError;
use ferrin_provider_util::sse::SseDecoder;
use ferrin_provider_util::sse::decode_stream;
use futures_util::StreamExt;
use futures_util::stream;

/// Events per synthetic body.
const EVENTS: usize = 2_000;
/// Per-event limit high enough that the limit check never trips.
const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// A body of `events` delta events shaped like a Responses API text stream,
/// terminated by `[DONE]`.
fn body(events: usize) -> Vec<u8> {
    let mut out = String::with_capacity(events * 170);
    for sequence in 0..events {
        out.push_str("event: response.output_text.delta\n");
        out.push_str(&format!(
            "data: {{\"type\":\"response.output_text.delta\",\"sequence_number\":{sequence},\
             \"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\
             \"delta\":\"token {sequence} \"}}\n\n"
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn chunks(body: &[u8], size: usize) -> Vec<Bytes> {
    body.chunks(size).map(Bytes::copy_from_slice).collect()
}

fn bytes(len: usize) -> Throughput {
    Throughput::Bytes(u64::try_from(len).unwrap())
}

/// Feeds every chunk and returns the number of decoded events.
fn feed_all(chunks: &[Bytes]) -> usize {
    let mut decoder = SseDecoder::new().with_max_event_bytes(MAX_EVENT_BYTES);
    let mut events = 0;
    for chunk in chunks {
        events += decoder.feed(chunk).unwrap().len();
    }
    decoder.finish();
    events
}

fn bench_feed(c: &mut Criterion) {
    let body = body(EVENTS);
    let mut group = c.benchmark_group("sse_decoder/feed");
    group.throughput(bytes(body.len()));
    for (label, size) in [
        ("whole_body", body.len()),
        ("4096", 4096),
        ("512", 512),
        ("64", 64),
    ] {
        let chunks = chunks(&body, size);
        group.bench_with_input(BenchmarkId::from_parameter(label), &chunks, |b, chunks| {
            b.iter(|| black_box(feed_all(chunks)));
        });
    }
    group.finish();
}

fn bench_decode_stream(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let body = body(EVENTS);
    let chunks = chunks(&body, 4096);
    let mut group = c.benchmark_group("sse_decoder/decode_stream");
    group.throughput(bytes(body.len()));
    group.bench_function("4096", |b| {
        b.to_async(&runtime).iter(|| {
            let chunks = chunks.clone();
            async move {
                let body =
                    stream::iter(chunks.into_iter().map(Ok::<Bytes, TransportError>)).boxed();
                black_box(decode_stream(body, MAX_EVENT_BYTES).count().await)
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_feed, bench_decode_stream);
criterion_main!(benches);
