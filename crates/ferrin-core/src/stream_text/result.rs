//! The result of `stream_text`: the event stream and the completion handle.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use ferrin_spec::BoxStream;
use ferrin_spec::PartId;
use futures_core::Stream;
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

/// Resolves with the final result once the event stream has been drained.
///
/// Dropping the event stream before it ends cancels the call; the
/// completion then resolves to [`Error::Cancelled`]. Awaiting the completion
/// without consuming the events stalls: the pipeline does not buffer.
pub struct Completion<O> {
    receiver: oneshot::Receiver<Result<GenerateTextResult<O>, Error>>,
}

impl<O> Completion<O> {
    pub(crate) fn new(receiver: oneshot::Receiver<Result<GenerateTextResult<O>, Error>>) -> Self {
        Self { receiver }
    }
}

impl<O> Future for Completion<O> {
    type Output = Result<GenerateTextResult<O>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Error::Cancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<O> fmt::Debug for Completion<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Completion(..)")
    }
}

/// Result of a streaming call: one event stream plus a completion handle.
///
/// The stream must be consumed for the call to progress. Use
/// [`split`](Self::split) to forward events in one task and await the
/// completion in another, or one of the consuming views.
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
        (self.events, self.completion)
    }

    /// The event stream.
    pub fn events(&mut self) -> &mut EventStream {
        &mut self.events
    }

    /// Drains every event and returns the final result.
    ///
    /// # Errors
    ///
    /// Returns the error that ended the call.
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error> {
        let (mut events, completion) = self.split();
        while events.next().await.is_some() {}
        completion.await
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
    /// Only the first text part of each step is parsed; a value is published
    /// whenever the repaired partial JSON changes. Errors are not reported
    /// here: use [`consume`](Self::consume) or the completion for them.
    pub fn partial_output_stream(self) -> impl Stream<Item = PartialOutput<O>> + Send {
        let state = PartialState {
            events: self.events,
            handler: self.output,
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
}

struct PartialState<O> {
    events: EventStream,
    handler: Arc<dyn OutputHandler<O>>,
    first_text: Option<PartId>,
    text: String,
    last: Option<String>,
}

impl<O: 'static> PartialState<O> {
    fn reset(&mut self) {
        self.first_text = None;
        self.text.clear();
        self.last = None;
    }

    fn handle(&mut self, event: &StreamEvent) -> Option<PartialOutput<O>> {
        match event {
            StreamEvent::StartStep { .. } | StreamEvent::RetryAttempt { .. } => {
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
                let serialized = value.to_string();
                if self.last.as_deref() == Some(serialized.as_str()) {
                    return None;
                }
                self.last = Some(serialized);
                let typed = self.handler.typed_partial(&value);
                Some(PartialOutput { value, typed })
            }
            _ => None,
        }
    }
}

impl<O> StreamTextResult<O>
where
    O: ArrayElements + Send + 'static,
    O::Element: DeserializeOwned + Send,
{
    /// Consumes the result, yielding each element of an array output as soon
    /// as it is complete. The final item is an error when the call failed.
    pub fn element_stream(self) -> impl Stream<Item = Result<O::Element, Error>> + Send {
        let state = ElementState {
            events: self.events,
            completion: Some(self.completion),
            handler: self.output,
            first_text: None,
            text: String::new(),
            published: 0,
            pending: VecDeque::new(),
        };
        stream::unfold(Some(state), |state| async move {
            let mut state = state?;
            loop {
                if let Some(item) = state.pending.pop_front() {
                    return Some((item, Some(state)));
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
}

struct ElementState<O: ArrayElements> {
    events: EventStream,
    completion: Option<Completion<O>>,
    handler: Arc<dyn OutputHandler<O>>,
    first_text: Option<PartId>,
    text: String,
    published: usize,
    pending: VecDeque<Result<O::Element, Error>>,
}

impl<O> ElementState<O>
where
    O: ArrayElements + 'static,
    O::Element: DeserializeOwned,
{
    fn handle(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::StartStep { .. } | StreamEvent::RetryAttempt { .. } => {
                self.first_text = None;
                self.text.clear();
                self.published = 0;
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
                let Some(elements) = self.handler.parse_elements(&self.text) else {
                    return;
                };
                for element in elements.into_iter().skip(self.published) {
                    self.published += 1;
                    self.pending
                        .push_back(serde_json::from_value(element).map_err(Error::other));
                }
            }
            _ => {}
        }
    }
}
