//! Schema derivation, the OpenAI strict-mode transform, and validation of a
//! JSON value both through `serde` (typed) and through the JSON Schema
//! validator (raw schema).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;

use criterion::BatchSize;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_schema::JsonSchema;
use ferrin_schema::Schema;
use ferrin_schema::SchemaTransform;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

/// Lines per order in the sample value.
const LINES: usize = 20;

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code, reason = "fields are only read by the validator")]
struct Order {
    /// Order identifier.
    id: String,
    customer: Customer,
    lines: Vec<Line>,
    notes: Option<String>,
    status: Status,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code, reason = "fields are only read by the validator")]
struct Customer {
    name: String,
    email: String,
    tier: u8,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code, reason = "fields are only read by the validator")]
struct Line {
    sku: String,
    quantity: u32,
    unit_price: f64,
    gift: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Status {
    Pending,
    Paid,
    Shipped,
}

fn order(lines: usize) -> Value {
    let lines: Vec<Value> = (0..lines)
        .map(|index| {
            json!({
                "sku": format!("SKU-{index:04}"),
                "quantity": index + 1,
                "unit_price": 9.99,
                "gift": index % 3 == 0,
            })
        })
        .collect();
    json!({
        "id": "order-1",
        "customer": { "name": "Ada", "email": "ada@example.com", "tier": 2 },
        "lines": lines,
        "notes": null,
        "status": "paid",
    })
}

fn bench_derived(c: &mut Criterion) {
    c.bench_function("schema/derived", |b| {
        b.iter(|| black_box(Schema::<Order>::derived()));
    });
}

fn bench_openai_strict(c: &mut Criterion) {
    let base = Schema::<Order>::derived().json_schema().clone();
    c.bench_function("schema/openai_strict", |b| {
        b.iter_batched(
            || base.clone(),
            |schema| black_box(SchemaTransform::openai_strict().applied(schema).unwrap()),
            BatchSize::SmallInput,
        );
    });
}

fn bench_validate(c: &mut Criterion) {
    let value = order(LINES);
    let typed = Schema::<Order>::derived();
    let raw = Schema::<Value>::from_json_schema(typed.json_schema().clone());
    let mut group = c.benchmark_group("schema/validate");
    group.bench_function("typed_serde", |b| {
        b.iter_batched(
            || value.clone(),
            |value| black_box(typed.validate(value).unwrap()),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("raw_json_schema", |b| {
        b.iter_batched(
            || value.clone(),
            |value| black_box(raw.validate(value).unwrap()),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_derived, bench_openai_strict, bench_validate);
criterion_main!(benches);
