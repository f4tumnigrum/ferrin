//! Pull-driven fan-out; the final cursor owns cancellation through upstream drop.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use futures_core::Stream;
use futures_util::task::ArcWake;
use futures_util::task::waker_ref;

use super::EventStream;
use super::StreamEvent;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
struct WakeViews(Mutex<BTreeMap<usize, Waker>>);

impl ArcWake for WakeViews {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let wakers: Vec<_> = lock(&arc_self.0).values().cloned().collect();
        for waker in wakers {
            waker.wake();
        }
    }
}

struct Shared {
    upstream: Option<EventStream>,
    queues: BTreeMap<usize, VecDeque<StreamEvent>>,
}

struct View {
    shared: Arc<Mutex<Shared>>,
    wake: Arc<WakeViews>,
    id: usize,
}

impl Stream for View {
    type Item = StreamEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        lock(&self.wake.0).insert(self.id, cx.waker().clone());
        let mut shared = lock(&self.shared);
        if let Some(event) = shared
            .queues
            .get_mut(&self.id)
            .and_then(VecDeque::pop_front)
        {
            return Poll::Ready(Some(event));
        }
        let Some(upstream) = shared.upstream.as_mut() else {
            return Poll::Ready(None);
        };
        let wake = waker_ref(&self.wake);
        match upstream.as_mut().poll_next(&mut Context::from_waker(&wake)) {
            Poll::Ready(Some(event)) => {
                for (id, queue) in &mut shared.queues {
                    if *id != self.id {
                        queue.push_back(event.clone());
                    }
                }
                drop(shared);
                WakeViews::wake_by_ref(&self.wake);
                Poll::Ready(Some(event))
            }
            Poll::Ready(None) => {
                shared.upstream = None;
                drop(shared);
                WakeViews::wake_by_ref(&self.wake);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        lock(&self.wake.0).remove(&self.id);
        lock(&self.shared).queues.remove(&self.id);
    }
}

pub(super) fn tee(upstream: EventStream) -> (EventStream, EventStream) {
    let shared = Arc::new(Mutex::new(Shared {
        upstream: Some(upstream),
        queues: BTreeMap::from([(0, VecDeque::new()), (1, VecDeque::new())]),
    }));
    let wake = Arc::new(WakeViews::default());
    (
        Box::pin(View {
            shared: shared.clone(),
            wake: wake.clone(),
            id: 0,
        }),
        Box::pin(View {
            shared,
            wake,
            id: 1,
        }),
    )
}
