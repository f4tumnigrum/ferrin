//! Drives provider-specific stream state machines and turns errors reported
//! before any output into request failures.
//!
//! Providers implement [`StreamMachine`] for their chunk type; [`drive_stream`]
//! turns the machine into a `StreamPart` stream that honours the
//! specification contract for terminal errors. [`fail_on_early_error`] peeks
//! at the first chunks of a freshly opened stream so that a server error
//! delivered inside an HTTP 200 body becomes an [`ProviderError`] the request
//! layer can retry.

use std::collections::VecDeque;
use std::time::Duration;

use ferrin_spec::BoxStream;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::ToolCallId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::StreamPart;
use futures_util::StreamExt;
use futures_util::stream;

use crate::http::ParseResult;

/// How long [`fail_on_early_error`] keeps waiting for output after the
/// server acknowledged the request without producing any.
pub const ACCEPTED_GRACE: Duration = Duration::from_millis(50);

/// How a chunk affects the early-error check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarlyChunk {
    /// The chunk carries an error frame.
    Error,
    /// The chunk carries model output; the check ends.
    Output,
    /// The server accepted the request but produced no output yet; the
    /// check waits at most [`ACCEPTED_GRACE`] for more chunks.
    Accepted,
    /// Anything else; buffered and replayed.
    Other,
}

/// Fails the request when the server reports an error before producing any
/// output, so the caller sees a request error instead of a stream that only
/// carries an error part.
///
/// `classify` inspects each parsed chunk; `to_error` converts the first
/// error chunk (typed value and raw JSON) into the error to return. Chunks
/// read by the check are replayed in front of the remaining stream; a parse
/// failure ends the check and is replayed as well.
///
/// # Errors
///
/// Returns the error produced by `to_error`.
pub async fn fail_on_early_error<T: Send + 'static>(
    stream: BoxStream<'static, ParseResult<T>>,
    classify: impl Fn(&T) -> EarlyChunk,
    to_error: impl FnOnce(&T, &JsonValue) -> ProviderError,
) -> Result<BoxStream<'static, ParseResult<T>>, ProviderError> {
    // Fused: the check may drain the stream completely, and the chained
    // replay below must not poll the underlying stream again afterwards.
    let mut stream = stream.fuse();
    let mut buffered: Vec<ParseResult<T>> = Vec::new();
    let mut accepted = false;
    loop {
        let next = if accepted {
            match tokio::time::timeout(ACCEPTED_GRACE, stream.next()).await {
                Ok(next) => next,
                Err(_) => break,
            }
        } else {
            stream.next().await
        };
        let Some(chunk) = next else {
            break;
        };
        let ParseResult::Ok { value, raw } = &chunk else {
            buffered.push(chunk);
            break;
        };
        match classify(value) {
            EarlyChunk::Error => return Err(to_error(value, raw)),
            EarlyChunk::Output => {
                buffered.push(chunk);
                break;
            }
            EarlyChunk::Accepted => {
                accepted = true;
                buffered.push(chunk);
            }
            EarlyChunk::Other => buffered.push(chunk),
        }
    }
    Ok(Box::pin(stream::iter(buffered).chain(stream)))
}

/// A per-chunk state machine that turns provider chunks into stream parts.
pub trait StreamMachine: Send + 'static {
    /// Provider chunk type.
    type Chunk: Send + 'static;

    /// Handles one chunk (or parse failure).
    fn handle(&mut self, chunk: ParseResult<Self::Chunk>, include_raw: bool) -> Vec<StreamPart>;

    /// Emits the closing parts once the chunk stream ended.
    fn finish(self) -> Vec<StreamPart>;
}

/// Open text, reasoning and tool-input parts, tracked so that an error can
/// close them before it terminates the stream.
#[derive(Debug, Default)]
struct OpenParts {
    text: Vec<PartId>,
    reasoning: Vec<PartId>,
    tool_inputs: Vec<ToolCallId>,
}

impl OpenParts {
    fn observe(&mut self, part: &StreamPart) {
        match part {
            StreamPart::TextStart { id, .. } => self.text.push(id.clone()),
            StreamPart::TextEnd { id, .. } => self.text.retain(|open| open != id),
            StreamPart::ReasoningStart { id, .. } => self.reasoning.push(id.clone()),
            StreamPart::ReasoningEnd { id, .. } => self.reasoning.retain(|open| open != id),
            StreamPart::ToolInputStart { id, .. } => self.tool_inputs.push(id.clone()),
            StreamPart::ToolInputEnd { id, .. } => self.tool_inputs.retain(|open| open != id),
            _ => {}
        }
    }

    /// Closing parts for everything still open, in opening order.
    fn close(&mut self) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        parts.extend(std::mem::take(&mut self.tool_inputs).into_iter().map(|id| {
            StreamPart::ToolInputEnd {
                id,
                provider_metadata: None,
            }
        }));
        parts.extend(std::mem::take(&mut self.reasoning).into_iter().map(|id| {
            StreamPart::ReasoningEnd {
                id,
                provider_metadata: None,
            }
        }));
        parts.extend(
            std::mem::take(&mut self.text)
                .into_iter()
                .map(|id| StreamPart::TextEnd {
                    id,
                    provider_metadata: None,
                }),
        );
        parts
    }
}

/// Drives a [`StreamMachine`] over `chunks`, emitting `start` first and the
/// machine's closing parts last.
///
/// An [`StreamPart::Error`] emitted by the machine is terminal: parts that
/// are still open are closed first, the error is forwarded, everything the
/// machine produced after it is dropped and the stream ends without a
/// `finish` part.
#[must_use]
pub fn drive_stream<M: StreamMachine>(
    start: StreamPart,
    chunks: BoxStream<'static, ParseResult<M::Chunk>>,
    machine: M,
    include_raw: bool,
) -> BoxStream<'static, StreamPart> {
    struct Driver<M: StreamMachine> {
        chunks: BoxStream<'static, ParseResult<M::Chunk>>,
        machine: Option<M>,
        pending: VecDeque<StreamPart>,
        open: OpenParts,
        include_raw: bool,
    }
    impl<M: StreamMachine> Driver<M> {
        /// Queues `parts`, terminating at the first error part.
        fn enqueue(&mut self, parts: Vec<StreamPart>) {
            for part in parts {
                if matches!(part, StreamPart::Error { .. }) {
                    self.pending.extend(self.open.close());
                    self.pending.push_back(part);
                    self.machine = None;
                    return;
                }
                self.open.observe(&part);
                self.pending.push_back(part);
            }
        }
    }
    let driver = Driver {
        chunks,
        machine: Some(machine),
        pending: VecDeque::from([start]),
        open: OpenParts::default(),
        include_raw,
    };
    Box::pin(stream::unfold(driver, |mut driver| async move {
        loop {
            if let Some(part) = driver.pending.pop_front() {
                return Some((part, driver));
            }
            driver.machine.as_ref()?;
            match driver.chunks.next().await {
                Some(chunk) => {
                    let include_raw = driver.include_raw;
                    let parts = driver.machine.as_mut()?.handle(chunk, include_raw);
                    driver.enqueue(parts);
                }
                None => {
                    let parts = driver.machine.take()?.finish();
                    driver.enqueue(parts);
                }
            }
        }
    }))
}
