//! PV-020: per-chunk cost of `Sleep::reset` for a chunk timeout versus a
//! timestamp-only design. Run: cargo run --release -p pv020-sleep-reset

use std::future::Future;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use std::time::Instant;

use tokio::time::Sleep;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let chunks = 1_000_000u32;
    let timeout = Duration::from_secs(5);
    let mut deadline: std::pin::Pin<Box<Sleep>> = Box::pin(sleep(timeout));
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    let start = Instant::now();
    for _ in 0..chunks {
        // register / re-register the timer as select! would
        assert!(matches!(deadline.as_mut().poll(&mut cx), Poll::Pending));
        deadline.as_mut().reset(tokio::time::Instant::now() + timeout);
    }
    let per_reset = start.elapsed() / chunks;
    let start = Instant::now();
    let mut last = Instant::now();
    for _ in 0..chunks {
        last = Instant::now();
    }
    let per_stamp = start.elapsed() / chunks;
    let _ = last;
    println!("chunks={chunks}: Sleep::poll+reset per chunk = {per_reset:?}; Instant::now per chunk = {per_stamp:?}");
}
