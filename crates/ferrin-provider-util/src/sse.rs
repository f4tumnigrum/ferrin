//! Server-sent events decoding (WHATWG EventSource).
//!
//! Handles `event`, `data`, `id` and `retry` fields, comment lines, the
//! three line terminators (`\r\n`, `\n`, `\r`), a leading UTF-8 BOM, and
//! enforces a per-event size limit. Incomplete trailing events are
//! discarded at end of stream, as the specification requires.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use futures_core::Stream;
use futures_util::StreamExt;

use crate::http::BodyStream;
use crate::http::TransportError;

/// Default maximum size of one event's accumulated data (16 MiB).
pub const DEFAULT_MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// One decoded event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Event type (`None` for the default `message` type).
    pub event: Option<String>,
    /// Data lines joined with `\n`.
    pub data: String,
    /// Last event id seen so far, if any.
    pub id: Option<String>,
    /// Reconnection time requested by the server.
    pub retry: Option<Duration>,
    /// When the event was received; set by [`decode_stream`].
    pub received_at: Option<Instant>,
}

/// Decoding failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SseError {
    /// An event exceeded the size limit.
    #[error("event exceeds the limit of {limit} bytes")]
    EventTooLarge {
        /// The configured limit.
        limit: usize,
    },
}

/// Incremental decoder: feed bytes, collect events.
#[derive(Debug)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    data: String,
    event: String,
    last_event_id: String,
    retry: Option<Duration>,
    bom_checked: bool,
    pending_cr: bool,
    max_event_bytes: usize,
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SseDecoder {
    /// Creates a decoder with the default event size limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            data: String::new(),
            event: String::new(),
            last_event_id: String::new(),
            retry: None,
            bom_checked: false,
            pending_cr: false,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
        }
    }

    /// Sets the maximum accumulated size of one event.
    #[must_use]
    pub fn with_max_event_bytes(mut self, max_event_bytes: usize) -> Self {
        self.max_event_bytes = max_event_bytes;
        self
    }

    /// Feeds bytes and returns the events completed by them.
    ///
    /// # Errors
    ///
    /// Returns [`SseError::EventTooLarge`] when the pending event exceeds the
    /// limit; the decoder should be discarded afterwards.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut events = Vec::new();
        let mut input = bytes;
        if !self.bom_checked && self.buffer.is_empty() {
            if input.len() >= 3 {
                self.bom_checked = true;
                if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    input = &input[3..];
                }
            } else if !input.is_empty() && input[0] != 0xEF {
                self.bom_checked = true;
            }
        }
        for &byte in input {
            if self.pending_cr {
                self.pending_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\n' | b'\r' => {
                    self.pending_cr = byte == b'\r';
                    let line = std::mem::take(&mut self.buffer);
                    if let Some(event) = self.process_line(&line) {
                        events.push(event);
                    }
                }
                other => {
                    self.buffer.push(other);
                    if self.buffer.len() + self.data.len() > self.max_event_bytes {
                        return Err(SseError::EventTooLarge {
                            limit: self.max_event_bytes,
                        });
                    }
                }
            }
        }
        if !self.bom_checked && self.buffer.len() >= 3 {
            self.bom_checked = true;
            if self.buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
                self.buffer.drain(..3);
            }
        }
        Ok(events)
    }

    /// Signals end of stream. Any incomplete event is discarded.
    pub fn finish(&mut self) {
        self.buffer.clear();
        self.data.clear();
        self.event.clear();
        self.pending_cr = false;
    }

    fn process_line(&mut self, line: &[u8]) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line[0] == b':' {
            return None;
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (text.as_ref(), ""),
        };
        match field {
            "event" => {
                self.event.clear();
                self.event.push_str(value);
            }
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" => {
                if !value.contains('\0') {
                    self.last_event_id.clear();
                    self.last_event_id.push_str(value);
                }
            }
            "retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                self.retry = value.parse::<u64>().ok().map(Duration::from_millis);
            }
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            self.event.clear();
            return None;
        }
        let mut data = std::mem::take(&mut self.data);
        data.pop();
        let event = std::mem::take(&mut self.event);
        Some(SseEvent {
            event: (!event.is_empty()).then_some(event),
            data,
            id: (!self.last_event_id.is_empty()).then(|| self.last_event_id.clone()),
            retry: self.retry,
            received_at: None,
        })
    }
}

/// Errors of [`decode_stream`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SseStreamError {
    /// The body stream failed.
    #[error(transparent)]
    Transport(TransportError),
    /// The bytes were not a valid event stream.
    #[error(transparent)]
    Decode(SseError),
}

/// Decodes a body stream into events, stamping `received_at`.
///
/// The stream ends after the first error.
pub fn decode_stream(
    body: BodyStream,
    max_event_bytes: usize,
) -> impl Stream<Item = Result<SseEvent, SseStreamError>> + Send + 'static {
    struct State {
        body: BodyStream,
        decoder: SseDecoder,
        pending: VecDeque<SseEvent>,
        done: bool,
    }
    let state = State {
        body,
        decoder: SseDecoder::new().with_max_event_bytes(max_event_bytes),
        pending: VecDeque::new(),
        done: false,
    };
    futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((Ok(event), state));
            }
            if state.done {
                return None;
            }
            match state.body.next().await {
                Some(Ok(chunk)) => match state.decoder.feed(&chunk) {
                    Ok(events) => {
                        let now = Instant::now();
                        state.pending.extend(events.into_iter().map(|mut event| {
                            event.received_at = Some(now);
                            event
                        }));
                    }
                    Err(error) => {
                        state.done = true;
                        return Some((Err(SseStreamError::Decode(error)), state));
                    }
                },
                Some(Err(error)) => {
                    state.done = true;
                    return Some((Err(SseStreamError::Transport(error)), state));
                }
                None => {
                    state.done = true;
                    state.decoder.finish();
                }
            }
        }
    })
}
