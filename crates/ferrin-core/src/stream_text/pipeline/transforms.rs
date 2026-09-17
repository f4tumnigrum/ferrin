//! Retry isolation for user stream transforms.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::task::Context;
use std::task::Poll;

use futures_core::Stream;
use futures_util::StreamExt;

use crate::stream_text::EventStream;
use crate::stream_text::StreamEvent;
use crate::stream_text::StreamTransform;
use crate::stream_text::TransformContext;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Source {
    input: EventStream,
    boundary: Option<StreamEvent>,
    ended: bool,
}

struct Segment(Arc<Mutex<Source>>);

impl Stream for Segment {
    type Item = StreamEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<StreamEvent>> {
        let mut source = lock(&self.0);
        if source.ended || source.boundary.is_some() {
            return Poll::Ready(None);
        }
        match source.input.as_mut().poll_next(cx) {
            Poll::Ready(Some(boundary @ StreamEvent::RetryAttempt { .. })) => {
                source.boundary = Some(boundary);
                Poll::Ready(None)
            }
            Poll::Ready(None) => {
                source.ended = true;
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

struct State {
    source: Arc<Mutex<Source>>,
    transforms: Vec<Arc<dyn StreamTransform>>,
    context: TransformContext,
    current: Option<EventStream>,
}

pub(super) fn apply(
    input: EventStream,
    transforms: Vec<Arc<dyn StreamTransform>>,
    context: TransformContext,
) -> EventStream {
    if transforms.is_empty() {
        return input;
    }
    let state = State {
        source: Arc::new(Mutex::new(Source {
            input,
            boundary: None,
            ended: false,
        })),
        transforms,
        context,
        current: None,
    };
    Box::pin(futures_util::stream::unfold(
        state,
        |mut state| async move {
            if state.current.is_none() {
                let mut segment: EventStream = Box::pin(Segment(state.source.clone()));
                for transform in &state.transforms {
                    segment = transform.apply(segment, state.context.clone());
                }
                state.current = Some(segment);
            }
            match state.current.as_mut()?.next().await {
                Some(event) => Some((event, state)),
                None => {
                    state.current = None;
                    let boundary = lock(&state.source).boundary.take();
                    // No boundary means either source EOF or intentional user
                    // truncation. Neither permits opening another segment.
                    boundary.map(|boundary| (boundary, state))
                }
            }
        },
    ))
}
