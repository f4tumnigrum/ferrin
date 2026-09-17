//! Retry segment isolation and smoothing failure/cancellation boundaries.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::StreamEvent;
use ferrin_core::stream_text;
use ferrin_core::stream_text::ErrorDecision;
use ferrin_core::stream_text::EventStream;
use ferrin_core::stream_text::StreamErrorInfo;
use ferrin_core::stream_text::TransformContext;
use ferrin_core::stream_text::smooth_stream;
use ferrin_core::stream_text::transforms::Chunking;
use ferrin_core::stream_text::transforms::SmoothStreamConfig;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::language_model::StreamError;
use ferrin_testing::text_parts;
use futures_util::FutureExt;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use regex::Regex;
use tokio_util::sync::CancellationToken;

use super::common::mock;

#[tokio::test]
async fn retry_boundaries_bypass_filters_and_recreate_transform_state() {
    let mut error = StreamError::new("retryable failure");
    error.is_retryable = Some(true);
    let mut failed = text_parts(["bad"], Usage::default());
    failed.pop();
    failed.push(StreamPart::Error { error });
    let model = mock()
        .stream(failed)
        .stream(text_parts(["good"], Usage::totals(1, 1)))
        .build_shared();
    let applications = Arc::new(AtomicUsize::new(0));
    let boundaries = Arc::new(AtomicUsize::new(0));
    let count = applications.clone();
    let observed = boundaries.clone();
    let result = stream_text(model)
        .prompt("hi")
        .stream_retries(1)
        .transform(
            move |input: EventStream, _ctx: TransformContext| -> EventStream {
                count.fetch_add(1, Ordering::SeqCst);
                let observed = observed.clone();
                let mut first = true;
                Box::pin(input.filter_map(move |mut event| {
                    let event = match &mut event {
                        StreamEvent::RetryAttempt { .. } => {
                            observed.fetch_add(1, Ordering::SeqCst);
                            None
                        }
                        StreamEvent::TextDelta { text, .. } if first => {
                            first = false;
                            *text = format!("first:{text}");
                            Some(event)
                        }
                        _ => Some(event),
                    };
                    std::future::ready(event)
                }))
            },
        )
        .await
        .unwrap();
    let (events, completion) = result.split();
    let result = tokio::time::timeout(Duration::from_secs(2), completion)
        .await
        .unwrap()
        .unwrap();
    let events: Vec<_> = events.collect().await;
    assert_eq!(
        (
            result.text(),
            applications.load(Ordering::SeqCst),
            boundaries.load(Ordering::SeqCst)
        ),
        ("first:good".to_owned(), 2, 0)
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, StreamEvent::RetryAttempt { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn smoothing_invalid_detector_lengths_and_empty_regex_matches_fail() {
    for chunking in [
        Chunking::detector(|_| Some(0)),
        Chunking::detector(|_| Some(99)),
        Chunking::detector(|_| Some(1)),
        Chunking::Regex(Regex::new("$").unwrap()),
    ] {
        let model = mock()
            .stream(text_parts(["é "], Usage::default()))
            .build_shared();
        let result = stream_text(model)
            .prompt("hi")
            .transform(smooth_stream(
                SmoothStreamConfig::new().chunking(chunking).delay(None),
            ))
            .await
            .unwrap();
        assert!(
            matches!(result.final_result().await, Err(Error::InvalidArgument { argument, .. }) if argument == "chunking")
        );
    }
}

#[tokio::test]
async fn transform_failure_is_returned_without_becoming_cancellation() {
    let model = mock()
        .stream(text_parts(["text"], Usage::default()))
        .build_shared();
    let result = stream_text(model)
        .prompt("hi")
        .transform(|input: EventStream, ctx: TransformContext| -> EventStream {
            Box::pin(input.take_while(move |_| {
                ctx.fail(Error::invalid_argument(
                    "transform",
                    "invalid transform configuration",
                ));
                std::future::ready(false)
            }))
        })
        .await
        .unwrap();
    assert!(
        matches!(result.final_result().await, Err(Error::InvalidArgument { argument, .. }) if argument == "transform")
    );
}

#[tokio::test(start_paused = true)]
async fn cancellation_interrupts_a_smoothing_delay() {
    let token = CancellationToken::new();
    let model = mock()
        .stream(text_parts(["first second "], Usage::default()))
        .build_shared();
    let result = stream_text(model)
        .prompt("hi")
        .cancellation(token.clone())
        .transform(smooth_stream(
            SmoothStreamConfig::new().delay(Some(Duration::from_secs(3600))),
        ))
        .await
        .unwrap();
    let (mut events, completion) = result.split();
    loop {
        if matches!(events.next().await, Some(StreamEvent::TextDelta { .. })) {
            break;
        }
    }
    assert!(events.next().now_or_never().is_none());
    let before = tokio::time::Instant::now();
    token.cancel();
    assert!(matches!(completion.await, Err(Error::Cancelled)));
    assert_eq!(
        tokio::time::Instant::now().duration_since(before),
        Duration::ZERO
    );
}

#[tokio::test]
async fn error_callback_panics_preserve_original_failure_and_retry_budget() {
    for asynchronous in [false, true] {
        let model = mock()
            .stream(vec![
                StreamPart::stream_start(),
                StreamPart::Error {
                    error: StreamError::new("provider failure"),
                },
            ])
            .build_shared();
        let result = stream_text(model)
            .prompt("hi")
            .on_error(move |_| {
                assert!(asynchronous, "synchronous callback panic");
                async {
                    panic!("asynchronous callback panic");
                }
            })
            .await
            .unwrap();
        let error = result.final_result().await.unwrap_err();
        assert!(!error.is_cancelled());
        assert!(error.to_string().contains("provider failure"));
    }
}

#[tokio::test]
async fn callback_can_retry_once_after_automatic_budget_but_not_when_omitted() {
    for automatic in [None, Some(0), Some(1)] {
        let failure = vec![
            StreamPart::stream_start(),
            StreamPart::Error {
                error: StreamError::new("provider failure"),
            },
        ];
        let model = mock()
            .stream(failure.clone())
            .stream(failure.clone())
            .stream(failure)
            .build_shared();
        let observed = Arc::new(AtomicUsize::new(0));
        let count = observed.clone();
        let mut call = stream_text(model.clone()).prompt("hi").on_error(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async { ErrorDecision::Retry }
        });
        if let Some(retries) = automatic {
            call = call.stream_retries(retries);
        }
        assert!(call.await.unwrap().final_result().await.is_err());
        let expected = automatic.map_or(1, |retries| usize::try_from(retries).unwrap() + 2);
        assert_eq!(
            (model.call_count(), observed.load(Ordering::SeqCst)),
            (expected, expected)
        );
    }
}

#[tokio::test]
async fn recovered_and_terminal_errors_notify_chunk_before_error_exactly_once() {
    for retries in [0, 1] {
        let failure = vec![
            StreamPart::stream_start(),
            StreamPart::Error {
                error: StreamError::new("provider failure"),
            },
        ];
        let model = mock()
            .stream(failure)
            .stream(text_parts(["good"], Usage::default()))
            .build_shared();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let chunks = log.clone();
        let errors = log.clone();
        let result = stream_text(model)
            .prompt("hi")
            .stream_retries(retries)
            .on_chunk(move |event: Arc<StreamEvent>| {
                let log = chunks.clone();
                async move {
                    if let StreamEvent::Error { error } = event.as_ref() {
                        log.lock().unwrap().push(format!("chunk:{}", error.message));
                    }
                }
            })
            .on_error(move |error: StreamErrorInfo| {
                let log = errors.clone();
                async move {
                    log.lock().unwrap().push(format!("error:{}", error.message));
                    ErrorDecision::Continue
                }
            })
            .await
            .unwrap();
        let outcome = result.final_result().await;
        assert_eq!(outcome.is_ok(), retries == 1);
        assert_eq!(
            log.lock().unwrap().clone(),
            vec![
                "chunk:stream error: provider failure",
                "error:stream error: provider failure"
            ]
        );
    }
}

#[tokio::test]
async fn transformed_errors_are_observed_after_the_original_retry_error() {
    let model = mock()
        .stream(vec![
            StreamPart::stream_start(),
            StreamPart::Error {
                error: StreamError::new("provider failure"),
            },
        ])
        .build_shared();
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let chunks = log.clone();
    let errors = log.clone();
    let result = stream_text(model)
        .prompt("hi")
        .transform(
            |input: EventStream, _ctx: TransformContext| -> EventStream {
                Box::pin(input.map(|mut event| {
                    if let StreamEvent::Error { error } = &mut event {
                        error.message = "rewritten".to_owned();
                    }
                    event
                }))
            },
        )
        .on_chunk(move |event: Arc<StreamEvent>| {
            let log = chunks.clone();
            async move {
                if let StreamEvent::Error { error } = event.as_ref() {
                    log.lock().unwrap().push(format!("chunk:{}", error.message));
                }
            }
        })
        .on_error(move |error: StreamErrorInfo| {
            let log = errors.clone();
            async move {
                log.lock().unwrap().push(format!("error:{}", error.message));
                ErrorDecision::Continue
            }
        })
        .await
        .unwrap();
    assert!(result.final_result().await.is_err());
    assert_eq!(
        log.lock().unwrap().clone(),
        vec![
            "chunk:stream error: provider failure",
            "error:stream error: provider failure",
            "chunk:rewritten",
            "error:rewritten"
        ]
    );
}

#[tokio::test]
async fn unicode_segmentation_emits_whitespace_separately_and_metadata_at_flush() {
    let mut parts = text_parts(["hello world!"], Usage::default());
    if let StreamPart::TextDelta {
        provider_metadata, ..
    } = &mut parts[2]
    {
        *provider_metadata = Some(
            serde_json::from_value(serde_json::json!({"test":{"signature":"fixture"}})).unwrap(),
        );
    }
    let result = stream_text(mock().stream(parts).build_shared())
        .prompt("hi")
        .transform(smooth_stream(
            SmoothStreamConfig::new()
                .chunking(Chunking::UnicodeWords)
                .delay(None),
        ))
        .await
        .unwrap();
    let (events, completion) = result.split();
    let deltas: Vec<_> = events
        .filter_map(|event| async move {
            match event {
                StreamEvent::TextDelta {
                    text,
                    provider_metadata,
                    ..
                } => Some((text, provider_metadata.is_some())),
                _ => None,
            }
        })
        .collect()
        .await;
    assert_eq!(
        deltas,
        vec![
            ("hello".to_owned(), false),
            (" ".to_owned(), false),
            ("world".to_owned(), false),
            ("!".to_owned(), false),
            (String::new(), true)
        ]
    );
    assert_eq!(completion.await.unwrap().text(), "hello world!");
}

#[tokio::test(start_paused = true)]
async fn an_oversized_smoothing_delay_can_be_cancelled_without_panicking() {
    let token = CancellationToken::new();
    let result = stream_text(
        mock()
            .stream(text_parts(["first second "], Usage::default()))
            .build_shared(),
    )
    .prompt("hi")
    .cancellation(token.clone())
    .transform(smooth_stream(
        SmoothStreamConfig::new().delay(Some(Duration::MAX)),
    ))
    .await
    .unwrap();
    let (mut events, completion) = result.split();
    loop {
        if matches!(events.next().await, Some(StreamEvent::TextDelta { .. })) {
            break;
        }
    }
    assert!(events.next().now_or_never().is_none());
    token.cancel();
    assert!(matches!(completion.await, Err(Error::Cancelled)));
}
