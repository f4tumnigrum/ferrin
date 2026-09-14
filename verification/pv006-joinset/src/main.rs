//! PV-006: 200 parallel "tools" via JoinSet, results injected into a bounded
//! mpsc channel consumed by a slow stream consumer. Measures ordering, peak
//! in-flight buffered results and wall time for several capacities.
//! Run: cargo run --release -p pv006-joinset -- <tools> <capacity>

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tools: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    let capacity: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let (tx, mut rx) = mpsc::channel::<(usize, Vec<u8>)>(capacity);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let start = Instant::now();
    let mut set = JoinSet::new();
    for i in 0..tools {
        let tx = tx.clone();
        let in_flight = Arc::clone(&in_flight);
        let peak = Arc::clone(&peak);
        set.spawn(async move {
            // simulated tool latency: 1..=20 ms, pseudo random by index
            tokio::time::sleep(Duration::from_millis(1 + (i as u64 * 7919) % 20)).await;
            let payload = vec![0u8; 4096]; // 4 KiB result
            let n = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            tx.send((i, payload)).await.expect("consumer alive");
        });
    }
    drop(tx);
    let consumer = {
        let in_flight = Arc::clone(&in_flight);
        tokio::spawn(async move {
            let mut order = Vec::with_capacity(tools);
            while let Some((i, _payload)) = rx.recv().await {
                in_flight.fetch_sub(1, Ordering::SeqCst);
                order.push(i);
                // slow consumer: 100 µs per item
                tokio::time::sleep(Duration::from_micros(100)).await;
            }
            order
        })
    };
    while let Some(res) = set.join_next().await {
        res.expect("task ok");
    }
    let order = consumer.await.expect("consumer ok");
    let elapsed = start.elapsed();
    let inversions = order.windows(2).filter(|w| w[0] > w[1]).count();
    println!(
        "tools={tools} capacity={capacity} elapsed={elapsed:?} received={} peak_buffered={} inversions_vs_spawn_order={inversions}",
        order.len(),
        peak.load(Ordering::SeqCst)
    );
}
