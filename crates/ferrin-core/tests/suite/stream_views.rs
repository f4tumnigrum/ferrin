//! Independent result views, completion driving and lifetime ownership.

use std::sync::Arc;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::Output;
use ferrin_core::StreamEvent;
use ferrin_core::stream_text;
use ferrin_schema::schemars;
use ferrin_spec::CallOptions;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::language_model::StreamResult;
use ferrin_testing::text_parts;
use futures_util::FutureExt;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::common::mock;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .expect("stream did not make progress")
}

#[tokio::test]
async fn final_result_drives_more_events_than_the_channel_capacity() {
    let model = mock()
        .stream(text_parts(
            std::iter::repeat_n("x", 200),
            Usage::totals(3, 5),
        ))
        .build_shared();
    let result = bounded(
        stream_text(Arc::clone(&model))
            .prompt("hi")
            .await
            .unwrap()
            .final_result(),
    )
    .await
    .unwrap();
    assert_eq!(
        (result.text(), result.total_usage.clone()),
        ("x".repeat(200), Usage::totals(3, 5))
    );
    assert_eq!(result.text(), "x".repeat(200));
    assert_eq!(model.call_count(), 1);
}

#[tokio::test]
async fn completion_drives_and_preserves_an_unpolled_event_branch() {
    let model = mock()
        .stream(text_parts(["a", "b"], Usage::totals(1, 2)))
        .build_shared();
    let (events, completion) = stream_text(model).prompt("hi").await.unwrap().split();
    let result = bounded(completion).await.unwrap();
    let events: Vec<_> = events.collect().await;
    assert_eq!(result.text(), "ab");
    assert_eq!(
        events
            .iter()
            .filter_map(StreamEvent::as_text_delta)
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(matches!(events.last(), Some(StreamEvent::Finish { .. })));
}

#[tokio::test]
async fn multiple_lagging_views_read_the_same_sequence_after_completion() {
    let model = mock()
        .stream(text_parts(
            std::iter::repeat_n("x", 130),
            Usage::totals(1, 2),
        ))
        .build_shared();
    let mut stream = stream_text(model).prompt("hi").await.unwrap();
    let first = stream.full_stream();
    let second = stream.full_stream();
    let text = stream.text_view();
    let result = bounded(stream.final_result()).await.unwrap();
    let first: Vec<_> = first.collect().await;
    let second: Vec<_> = second.collect().await;
    let text: String = text.collect::<Vec<_>>().await.concat();
    assert_eq!(first, second);
    assert_eq!(text, result.text());
}

#[tokio::test]
async fn concurrently_polled_views_and_completion_wake_each_other() {
    let (send, receive) = oneshot::channel();
    let receive = Arc::new(std::sync::Mutex::new(Some(receive)));
    let model = mock()
        .stream_with(move |_: CallOptions| {
            let receive = receive.lock().unwrap().take().unwrap();
            async move {
                let head = futures_util::stream::iter(vec![StreamPart::stream_start()]);
                let tail = futures_util::stream::once(async move {
                    receive.await.unwrap();
                    text_parts(["ready"], Usage::totals(1, 2))
                        .into_iter()
                        .skip(1)
                        .collect::<Vec<_>>()
                })
                .flat_map(futures_util::stream::iter);
                Ok(StreamResult::new(Box::pin(head.chain(tail))))
            }
        })
        .build_shared();
    let mut stream = stream_text(model).prompt("hi").await.unwrap();
    let text = stream.text_view();
    let full = stream.full_stream();
    let completion = stream.into_completion();
    let drive = async {
        let text = text.collect::<Vec<_>>();
        let full = full.collect::<Vec<_>>();
        tokio::pin!(text, full, completion);
        assert!(text.as_mut().now_or_never().is_none());
        assert!(full.as_mut().now_or_never().is_none());
        assert!(completion.as_mut().now_or_never().is_none());
        send.send(()).unwrap();
        tokio::join!(text, full, completion)
    };
    let (text, full, final_result) = bounded(drive).await;
    assert_eq!(text, vec!["ready"]);
    assert!(matches!(full.last(), Some(StreamEvent::Finish { .. })));
    assert_eq!(final_result.unwrap().text(), "ready");
}

#[tokio::test]
async fn dropping_one_branch_keeps_other_owners_running() {
    let model = mock()
        .stream(text_parts(["kept"], Usage::totals(1, 1)))
        .build_shared();
    let (events, completion) = stream_text(model).prompt("hi").await.unwrap().split();
    drop(events);
    assert_eq!(bounded(completion).await.unwrap().text(), "kept");
    let model = mock()
        .stream(text_parts(["kept"], Usage::totals(1, 1)))
        .build_shared();
    let (events, completion) = stream_text(model).prompt("hi").await.unwrap().split();
    drop(completion);
    let events: Vec<_> = bounded(events.collect()).await;
    assert!(matches!(events.last(), Some(StreamEvent::Finish { .. })));
}

struct DropNotice(Option<oneshot::Sender<()>>);
impl Drop for DropNotice {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn final_owner_drop_releases_a_pending_provider_stream() {
    let (send, receive) = oneshot::channel();
    let notice = Arc::new(std::sync::Mutex::new(Some(DropNotice(Some(send)))));
    let model = mock()
        .stream_with(move |_: CallOptions| {
            let notice = notice.lock().unwrap().take().unwrap();
            async move {
                let stream =
                    futures_util::stream::unfold((notice, false), |(notice, started)| async move {
                        if started {
                            std::future::pending::<()>().await;
                        }
                        Some((StreamPart::stream_start(), (notice, true)))
                    });
                Ok(StreamResult::new(Box::pin(stream)))
            }
        })
        .build_shared();
    let mut stream = stream_text(model).prompt("hi").await.unwrap();
    let view = stream.full_stream();
    let completion = stream.into_completion();
    drop(completion);
    let mut receive = receive;
    assert!((&mut receive).now_or_never().is_none());
    drop(view);
    bounded(receive).await.unwrap();
}

#[tokio::test]
async fn caller_cancellation_reaches_completion_and_every_view() {
    let token = CancellationToken::new();
    let model = mock()
        .stream_with(|_: CallOptions| async {
            Ok(StreamResult::new(Box::pin(
                futures_util::stream::iter([StreamPart::stream_start()])
                    .chain(futures_util::stream::pending()),
            )))
        })
        .build_shared();
    let mut stream = stream_text(model)
        .prompt("hi")
        .cancellation(token.clone())
        .await
        .unwrap();
    let events = stream.full_stream();
    token.cancel();
    assert!(matches!(
        bounded(stream.final_result()).await,
        Err(Error::Cancelled)
    ));
    let events: Vec<_> = events.collect().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Abort))
    );
}

#[tokio::test]
async fn terminal_errors_remain_visible_to_views_and_completion() {
    let model = mock()
        .stream(vec![
            StreamPart::stream_start(),
            StreamPart::Error {
                error: StreamError::new("failure"),
            },
        ])
        .build_shared();
    let (events, completion) = stream_text(model).prompt("hi").await.unwrap().split();
    assert!(bounded(completion).await.is_err());
    let events: Vec<_> = events.collect().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Error { .. }))
    );
}

#[derive(Debug, PartialEq, Deserialize, schemars::JsonSchema)]
struct Item {
    value: u32,
}

#[tokio::test]
async fn structured_views_and_final_output_share_one_call() {
    let model = mock()
        .stream(text_parts(
            ["{\"elements\":[{\"value\":1}", ",{\"value\":2}]}"],
            Usage::totals(1, 1),
        ))
        .build_shared();
    let mut stream = stream_text(Arc::clone(&model))
        .prompt("hi")
        .output(Output::<Vec<Item>>::array())
        .await
        .unwrap();
    let partial = stream.partial_output_view();
    let elements = stream.element_view();
    let result = bounded(stream.final_result()).await.unwrap();
    let partial: Vec<_> = partial.collect().await;
    let elements: Vec<_> = elements.map(Result::unwrap).collect().await;
    assert!(!partial.is_empty());
    assert_eq!(elements, vec![Item { value: 1 }, Item { value: 2 }]);
    assert_eq!(result.output, elements);
    assert_eq!(model.call_count(), 1);
}

#[tokio::test]
async fn shared_completion_waiters_reuse_the_same_nonclone_result() {
    let model = mock()
        .stream(text_parts(["{\"value\":7}"], Usage::totals(1, 2)))
        .build_shared();
    let waiter = stream_text(model)
        .prompt("hi")
        .output(Output::<Item>::object())
        .await
        .unwrap()
        .into_shared_completion();
    let another = waiter.clone();
    let (first, second) = bounded(async { tokio::join!(waiter, another) }).await;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.as_ref().as_ref().unwrap().output, Item { value: 7 });
}
