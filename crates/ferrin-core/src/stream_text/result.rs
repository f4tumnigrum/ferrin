//! The result of `stream_text`: the event stream and the completion handle.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

mod tee;

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use ferrin_spec::BoxStream;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TypeValidationError;
use futures_core::Stream;
use futures_util::FutureExt;
use futures_util::StreamExt;
use futures_util::stream;
use serde::de::DeserializeOwned;
use tokio::sync::oneshot;

use super::StreamEvent;
use crate::error::Error;
use crate::generate_text::GenerateTextResult;
use crate::output::ArrayElements;
use crate::output::OutputHandler;
use crate::output::PartialOutput;

/// A boxed stream of [`StreamEvent`]s.
pub type EventStream = BoxStream<'static, StreamEvent>;

/// Drives the event pipeline and resolves with its final result.
///
/// Other event views remain readable while this future runs or after it
/// completes. Unread events are buffered for those views; drop unused views
/// to release their buffers. Dropping every view and this future cancels the
/// owned pipeline. The final result is owned and does not require `O: Clone`.
pub struct Completion<O> {
    receiver: oneshot::Receiver<Result<GenerateTextResult<O>, Error>>,
    driver: Option<EventStream>,
}

impl<O> Completion<O> {
    pub(crate) fn new(receiver: oneshot::Receiver<Result<GenerateTextResult<O>, Error>>) -> Self {
        Self {
            receiver,
            driver: None,
        }
    }

    fn driving(mut self, driver: EventStream) -> Self {
        self.driver = Some(driver);
        self
    }
}

impl<O> Future for Completion<O> {
    type Output = Result<GenerateTextResult<O>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Bound synchronous draining so an already-buffered stream cannot
        // monopolize the executor while other cursors wait for progress.
        for _ in 0..64 {
            match Pin::new(&mut self.receiver).poll(cx) {
                Poll::Ready(Ok(result)) => {
                    self.driver = None;
                    return Poll::Ready(result);
                }
                Poll::Ready(Err(_)) => {
                    self.driver = None;
                    return Poll::Ready(Err(Error::Cancelled));
                }
                Poll::Pending => {}
            }
            let Some(driver) = self.driver.as_mut() else {
                return Poll::Pending;
            };
            match driver.as_mut().poll_next(cx) {
                Poll::Ready(Some(_)) => {}
                Poll::Ready(None) => {
                    self.driver = None;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<O> fmt::Debug for Completion<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Completion(..)")
    }
}

/// Result of a streaming call with independent event views and a final result.
///
/// Reading a view or awaiting [`final_result`](Self::final_result) drives
/// progress. Views created through [`full_stream`](Self::full_stream) keep
/// their unread events until consumed or dropped; a slow view does not
/// block another view. The final owner drop cancels the pipeline.
pub struct StreamTextResult<O> {
    pub(crate) call_id: String,
    pub(crate) events: EventStream,
    pub(crate) completion: Completion<O>,
    pub(crate) output: Arc<dyn OutputHandler<O>>,
}

impl<O> fmt::Debug for StreamTextResult<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTextResult")
            .field("call_id", &self.call_id)
            .finish_non_exhaustive()
    }
}

impl<O> StreamTextResult<O> {
    /// The call id.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Splits the result into the event stream and the completion handle.
    #[must_use]
    pub fn split(self) -> (EventStream, Completion<O>) {
        let (events, driver) = tee::tee(self.events);
        (events, self.completion.driving(driver))
    }

    /// Creates an independent view starting at the current event cursor.
    ///
    /// Creating a view leaves the result's cursor in place, allowing later
    /// views to read the same events. Lagging views retain unread events.
    #[must_use]
    pub fn full_stream(&mut self) -> EventStream {
        let current = std::mem::replace(&mut self.events, Box::pin(stream::empty()));
        let (view, retained) = tee::tee(current);
        self.events = retained;
        view
    }

    /// Returns a completion future that drives the pipeline without an event consumer.
    #[must_use]
    pub fn into_completion(self) -> Completion<O> {
        self.completion.driving(self.events)
    }

    /// Drives the pipeline and returns its owned final result.
    ///
    /// # Errors
    ///
    /// Returns the error that ended the call. Retain the result to borrow its
    /// text, usage, steps and output repeatedly after successful completion.
    pub async fn final_result(self) -> Result<GenerateTextResult<O>, Error> {
        self.into_completion().await
    }

    /// Creates an independent view yielding only text deltas.
    ///
    /// Inspect a full view or await the final result for terminal errors.
    pub fn text_view(&mut self) -> impl Stream<Item = String> + Send + use<O> {
        self.full_stream().filter_map(|event| async move {
            match event {
                StreamEvent::TextDelta { text, .. } => Some(text),
                _ => None,
            }
        })
    }

    /// The result's current event cursor; consuming it advances future view starts.
    pub fn events(&mut self) -> &mut EventStream {
        &mut self.events
    }

    /// Drains every event and returns the final result.
    ///
    /// # Errors
    ///
    /// Returns the error that ended the call.
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error> {
        self.final_result().await
    }
}

impl<O: Send + Sync + 'static> StreamTextResult<O> {
    /// Returns cloneable final-result waiters that jointly drive the pipeline.
    ///
    /// The result and error are shared through `Arc`, so neither must be
    /// cloneable. Each waiter resolves to the same allocation. Dropping all
    /// waiters and event views cancels an unfinished call.
    pub fn into_shared_completion(
        self,
    ) -> impl Future<Output = Arc<Result<GenerateTextResult<O>, Error>>> + Clone + Send {
        self.into_completion().map(Arc::new).boxed().shared()
    }
}

impl<O: Send + 'static> StreamTextResult<O> {
    /// Consumes the result, yielding text deltas only. The final item is an
    /// error when the call failed.
    pub fn text_stream(self) -> impl Stream<Item = Result<String, Error>> + Send {
        stream::unfold(Some((self.events, self.completion)), |state| async move {
            let (mut events, completion) = state?;
            loop {
                match events.next().await {
                    Some(StreamEvent::TextDelta { text, .. }) => {
                        return Some((Ok(text), Some((events, completion))));
                    }
                    Some(_) => {}
                    None => {
                        return match completion.await {
                            Ok(_) => None,
                            Err(error) => Some((Err(error), None)),
                        };
                    }
                }
            }
        })
    }

    /// Consumes the result, yielding the structured output as it grows.
    ///
    /// Only the first text part of the call (or retry attempt) is parsed; a value is published
    /// whenever the repaired partial JSON changes. Errors are not reported
    /// here: use [`consume`](Self::consume) or the completion for them.
    pub fn partial_output_stream(self) -> impl Stream<Item = PartialOutput<O>> + Send {
        partial_stream(self.events, self.output)
    }

    /// Creates an independent view of changing structured output.
    ///
    /// Like the consuming view, this parses the first text part per attempt;
    /// terminal errors remain available through the final result.
    pub fn partial_output_view(&mut self) -> impl Stream<Item = PartialOutput<O>> + Send + use<O> {
        partial_stream(self.full_stream(), self.output.clone())
    }
}

fn partial_stream<O: Send + 'static>(
    events: EventStream,
    handler: Arc<dyn OutputHandler<O>>,
) -> impl Stream<Item = PartialOutput<O>> + Send {
    let state = PartialState {
        events,
        handler,
        first_text: None,
        text: String::new(),
        last: None,
    };
    stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        loop {
            let event = state.events.next().await?;
            if let Some(output) = state.handle(&event) {
                return Some((output, Some(state)));
            }
        }
    })
}

struct PartialState<O> {
    events: EventStream,
    handler: Arc<dyn OutputHandler<O>>,
    first_text: Option<PartId>,
    text: String,
    last: Option<JsonValue>,
}

impl<O: 'static> PartialState<O> {
    fn reset(&mut self) {
        self.first_text = None;
        self.text.clear();
        self.last = None;
    }

    fn handle(&mut self, event: &StreamEvent) -> Option<PartialOutput<O>> {
        match event {
            StreamEvent::RetryAttempt { .. } => {
                self.reset();
                None
            }
            StreamEvent::TextStart { id, .. } => {
                if self.first_text.is_none() {
                    self.first_text = Some(id.clone());
                }
                None
            }
            StreamEvent::TextDelta { id, text, .. } => {
                if self.first_text.as_ref() != Some(id) || text.is_empty() {
                    return None;
                }
                self.text.push_str(text);
                let value = self.handler.parse_partial(&self.text)?;
                if self.last.as_ref() == Some(&value) {
                    return None;
                }
                self.last = Some(value.clone());
                let typed = self.handler.typed_partial(&value);
                Some(PartialOutput { value, typed })
            }
            _ => None,
        }
    }
}

impl<O> StreamTextResult<O>
where
    O: ArrayElements + IntoIterator<Item = <O as ArrayElements>::Element> + Send + 'static,
    O::Element: DeserializeOwned + Send,
{
    /// Consumes the result, yielding each element of an array output as soon
    /// as it is complete. The final item is an error when the call failed.
    pub fn element_stream(self) -> impl Stream<Item = Result<O::Element, Error>> + Send {
        elements(self.events, Some(self.completion), self.output)
    }

    /// Creates an independent view of completed array elements.
    ///
    /// Element decoding errors appear in this view; inspect the final result
    /// for terminal provider or pipeline errors.
    pub fn element_view(
        &mut self,
    ) -> impl Stream<Item = Result<O::Element, Error>> + Send + use<O> {
        elements(self.full_stream(), None, self.output.clone())
    }
}

fn elements<O>(
    events: EventStream,
    completion: Option<Completion<O>>,
    handler: Arc<dyn OutputHandler<O>>,
) -> impl Stream<Item = Result<O::Element, Error>> + Send
where
    O: ArrayElements + IntoIterator<Item = <O as ArrayElements>::Element> + Send + 'static,
    O::Element: DeserializeOwned + Send,
{
    let state = ElementState {
        events,
        completion,
        handler,
        first_text: None,
        text: String::new(),
        published: 0,
        pending: VecDeque::new(),
        failed: false,
    };
    stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        loop {
            if let Some(item) = state.pending.pop_front() {
                let next = if state.failed && state.pending.is_empty() {
                    None
                } else {
                    Some(state)
                };
                return Some((item, next));
            }
            match state.events.next().await {
                Some(event) => state.handle(&event),
                None => {
                    let completion = state.completion.take()?;
                    return match completion.await {
                        Ok(_) => None,
                        Err(error) => Some((Err(error), None)),
                    };
                }
            }
        }
    })
}

struct ElementState<O: ArrayElements> {
    events: EventStream,
    completion: Option<Completion<O>>,
    handler: Arc<dyn OutputHandler<O>>,
    first_text: Option<PartId>,
    text: String,
    published: usize,
    pending: VecDeque<Result<O::Element, Error>>,
    failed: bool,
}

impl<O> ElementState<O>
where
    O: ArrayElements + IntoIterator<Item = <O as ArrayElements>::Element> + 'static,
    O::Element: DeserializeOwned,
{
    fn handle(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::RetryAttempt { .. } => {
                self.first_text = None;
                self.text.clear();
                // Already-published array prefixes cannot be retracted. Keep
                // the count, as in the reference element transform.
            }
            StreamEvent::TextStart { id, .. } => {
                if self.first_text.is_none() {
                    self.first_text = Some(id.clone());
                }
            }
            StreamEvent::TextDelta { id, text, .. } => {
                if self.first_text.as_ref() != Some(id) || text.is_empty() {
                    return;
                }
                self.text.push_str(text);
                let elements: Vec<Result<O::Element, Error>> =
                    if let Some(typed) = self.handler.parse_typed_elements(&self.text) {
                        typed.into_iter().map(Ok).collect()
                    } else if let Some(raw) = self.handler.parse_elements(&self.text) {
                        raw.into_iter()
                            .map(|value| serde_json::from_value(value).map_err(Error::other))
                            .collect()
                    } else {
                        return;
                    };
                for element in elements.into_iter().skip(self.published) {
                    if let Some(max) = self.handler.max_elements()
                        && self.published >= max
                    {
                        let value = self
                            .handler
                            .parse_partial(&self.text)
                            .unwrap_or(JsonValue::Null);
                        let error = TypeValidationError::new(
                            value,
                            std::io::Error::other(format!(
                                "elements array must contain at most {max} items"
                            )),
                        );
                        self.pending
                            .push_back(Err(Error::from(ProviderError::from(error))));
                        self.failed = true;
                        break;
                    }
                    self.published += 1;
                    self.pending.push_back(element);
                }
            }
            _ => {}
        }
    }
}
