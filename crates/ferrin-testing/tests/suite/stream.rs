use std::time::Duration;

use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_testing::SimulatedStream;
use ferrin_testing::simulate_stream;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn simulate_stream_yields_parts_in_order() {
    let parts = text_parts(["a", "b"], Usage::totals(1, 2));
    let result = simulate_stream(parts.clone());
    let collected: Vec<StreamPart> = result.stream.collect().await;
    assert_eq!(collected, parts);
}

#[tokio::test(start_paused = true)]
async fn delays_apply_before_first_and_between_parts() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::text_delta(PartId::new("0"), "x"),
        StreamPart::finish(FinishReason::stop(), Usage::totals(0, 0)),
    ];
    let result = SimulatedStream::new(parts)
        .initial_delay(Duration::from_millis(100))
        .chunk_delay(Duration::from_millis(50))
        .build();
    let start = tokio::time::Instant::now();
    let mut stream = result.stream;
    stream.next().await.unwrap();
    assert_eq!(start.elapsed(), Duration::from_millis(100));
    stream.next().await.unwrap();
    assert_eq!(start.elapsed(), Duration::from_millis(150));
    stream.next().await.unwrap();
    assert_eq!(start.elapsed(), Duration::from_millis(200));
    assert!(stream.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn hang_at_end_never_completes() {
    let result = SimulatedStream::new(vec![StreamPart::stream_start()])
        .hang_at_end()
        .build();
    let mut stream = result.stream;
    stream.next().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(60), stream.next()).await;
    assert!(outcome.is_err(), "the stream must not end");
}
