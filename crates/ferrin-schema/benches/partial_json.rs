//! Partial JSON: `repair` and `parse_partial` on prefixes of a realistic
//! streamed document, with `serde_json` on the complete text as the baseline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_schema::partial_json::parse_partial;
use ferrin_schema::partial_json::repair;
use serde_json::Value;
use serde_json::json;

/// Items in the synthetic document (about 4 KiB in total).
const ITEMS: usize = 24;

/// Truncation points, in percent of the document length.
const CUTS: [usize; 4] = [25, 50, 75, 100];

/// A catalog document with nested objects, arrays, numbers, booleans and
/// escaped strings.
fn document(items: usize) -> String {
    let items: Vec<Value> = (0..items)
        .map(|index| {
            json!({
                "id": format!("item-{index}"),
                "name": format!("Item number {index} with a descriptive name"),
                "tags": ["alpha", "beta", "gamma"],
                "price": 12.5 + f64::from(u8::try_from(index % 200).unwrap()),
                "available": index % 2 == 0,
                "dimensions": { "width": index, "height": index * 2, "unit": "cm" },
                "notes": "Line one.\nLine \"two\" with an escaped quote and a unicode tab \u{2192}."
            })
        })
        .collect();
    json!({
        "title": "Catalog",
        "generated_at": "2026-09-14T00:00:00Z",
        "total": items.len(),
        "items": items,
    })
    .to_string()
}

/// The first `percent` of `text`, cut on a character boundary.
fn prefix(text: &str, percent: usize) -> &str {
    let mut cut = text.len() * percent / 100;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

fn bytes(len: usize) -> Throughput {
    Throughput::Bytes(u64::try_from(len).unwrap())
}

fn bench_repair(c: &mut Criterion) {
    let document = document(ITEMS);
    let mut group = c.benchmark_group("partial_json/repair");
    for percent in CUTS {
        let input = prefix(&document, percent);
        group.throughput(bytes(input.len()));
        group.bench_with_input(BenchmarkId::from_parameter(percent), input, |b, input| {
            b.iter(|| black_box(repair(input).len()));
        });
    }
    group.finish();
}

fn bench_parse_partial(c: &mut Criterion) {
    let document = document(ITEMS);
    let mut group = c.benchmark_group("partial_json/parse_partial");
    for percent in CUTS {
        let input = prefix(&document, percent);
        group.throughput(bytes(input.len()));
        group.bench_with_input(BenchmarkId::from_parameter(percent), input, |b, input| {
            b.iter(|| black_box(parse_partial(input).value.is_some()));
        });
    }
    group.finish();
}

fn bench_serde_baseline(c: &mut Criterion) {
    let document = document(ITEMS);
    let mut group = c.benchmark_group("partial_json/serde_json_complete");
    group.throughput(bytes(document.len()));
    group.bench_function("100", |b| {
        b.iter(|| black_box(serde_json::from_str::<Value>(&document).unwrap()));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_repair,
    bench_parse_partial,
    bench_serde_baseline
);
criterion_main!(benches);
