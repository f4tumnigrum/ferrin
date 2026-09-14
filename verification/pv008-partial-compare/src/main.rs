//! PV-008: cost of deep `serde_json::Value` equality per streamed delta versus
//! hashing the canonical serialization. Run: cargo run --release -p pv008-partial-compare

use std::hash::Hash;
use std::hash::Hasher;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;

fn big_object(entries: usize) -> Value {
    let items: Vec<Value> = (0..entries)
        .map(|i| json!({"id": i, "name": format!("item-{i}"), "tags": ["a", "b", "c"], "score": i as f64 * 0.5, "nested": {"x": i, "y": [i, i + 1]}}))
        .collect();
    json!({"items": items, "meta": {"count": entries}})
}

fn hash_value(v: &Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap().hash(&mut hasher);
    hasher.finish()
}

fn main() {
    for entries in [10usize, 100, 1_000, 5_000] {
        let a = big_object(entries);
        let b = a.clone();
        let text_len = serde_json::to_string(&a).unwrap().len();
        let iters = 2_000usize.max(20_000 / entries.max(1)).min(20_000);
        let t = Instant::now();
        let mut eq = 0usize;
        for _ in 0..iters {
            if a == b {
                eq += 1;
            }
        }
        let eq_ns = t.elapsed().as_nanos() as f64 / iters as f64;
        let t = Instant::now();
        let mut same = 0usize;
        for _ in 0..iters {
            if hash_value(&a) == hash_value(&b) {
                same += 1;
            }
        }
        let hash_ns = t.elapsed().as_nanos() as f64 / iters as f64 / 2.0;
        let t = Instant::now();
        let text = serde_json::to_string(&a).unwrap();
        for _ in 0..iters {
            let _: Value = serde_json::from_str(&text).unwrap();
        }
        let parse_ns = t.elapsed().as_nanos() as f64 / iters as f64;
        println!(
            "entries={entries:>5} json_bytes={text_len:>8} value_eq={eq_ns:>10.0} ns  serialize+hash={hash_ns:>10.0} ns  parse={parse_ns:>10.0} ns  (eq={eq} same={same})"
        );
    }
}
