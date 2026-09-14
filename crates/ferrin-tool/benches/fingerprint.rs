//! Tool fingerprints: canonical JSON, fingerprinting a tool set and drift
//! detection between two fingerprint sets.

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
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use ferrin_tool::fingerprint::canonical_json;
use ferrin_tool::fingerprint::detect_tool_drift;
use ferrin_tool::fingerprint::fingerprint_tools;
use serde_json::Value;
use serde_json::json;

/// Tool set sizes.
const SIZES: [usize; 2] = [5, 20];

/// An object schema with `properties` string/number/array properties.
fn schema(properties: usize, marker: &str) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for index in 0..properties {
        let name = format!("field_{index}");
        let property = match index % 3 {
            0 => json!({ "type": "string", "description": format!("Field {index} ({marker})") }),
            1 => json!({ "type": "integer", "minimum": 0, "maximum": 100 }),
            _ => json!({ "type": "array", "items": { "type": "string" } }),
        };
        props.insert(name.clone(), property);
        required.push(Value::String(name));
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool(index: usize, marker: &str) -> Tool {
    Tool::function_with_schema(Schema::from_json_schema(schema(3 + index % 5, marker)))
        .description(format!("Tool number {index} used by the benchmark."))
        .execute(|input: Value, _ctx: ToolContext| async move { Ok::<_, ToolError>(input) })
        .build()
}

fn tool_set(size: usize, marker: &str) -> ToolSet {
    (0..size).fold(ToolSet::new(), |set, index| {
        set.insert(format!("tool_{index}").as_str(), tool(index, marker))
            .unwrap()
    })
}

fn bench_canonical_json(c: &mut Criterion) {
    let value = schema(40, "canonical");
    let mut group = c.benchmark_group("fingerprint/canonical_json");
    group.throughput(Throughput::Bytes(
        u64::try_from(value.to_string().len()).unwrap(),
    ));
    group.bench_function("40_properties", |b| {
        b.iter(|| black_box(canonical_json(&value).len()));
    });
    group.finish();
}

fn bench_fingerprint_tools(c: &mut Criterion) {
    let mut group = c.benchmark_group("fingerprint/fingerprint_tools");
    for size in SIZES {
        let tools = tool_set(size, "baseline");
        group.throughput(Throughput::Elements(u64::try_from(size).unwrap()));
        group.bench_with_input(BenchmarkId::from_parameter(size), &tools, |b, tools| {
            b.iter(|| black_box(fingerprint_tools(tools)));
        });
    }
    group.finish();
}

fn bench_detect_drift(c: &mut Criterion) {
    let baseline = fingerprint_tools(&tool_set(20, "baseline"));
    let current = fingerprint_tools(&tool_set(20, "changed"));
    c.bench_function("fingerprint/detect_tool_drift/20", |b| {
        b.iter(|| black_box(detect_tool_drift(&current, &baseline)));
    });
}

criterion_group!(
    benches,
    bench_canonical_json,
    bench_fingerprint_tools,
    bench_detect_drift
);
criterion_main!(benches);
