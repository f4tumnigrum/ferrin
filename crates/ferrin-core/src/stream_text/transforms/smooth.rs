//! `smooth_stream`: buffers text and reasoning deltas and re-emits them in
//! word- or line-sized chunks with a small delay, producing an even typing
//! rhythm regardless of provider chunking.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use futures_util::StreamExt;
use futures_util::stream;
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

use super::StreamTransform;
use super::TransformContext;
use crate::error::Error;
use crate::stream_text::EventStream;
use crate::stream_text::StreamErrorInfo;
use crate::stream_text::StreamEvent;

/// A function returning the byte length of the next chunk in a buffer, or
/// `None` when the buffer holds no complete chunk yet.
pub type ChunkDetector = Arc<dyn Fn(&str) -> Option<usize> + Send + Sync>;

/// How buffered text is split into chunks.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum Chunking {
    /// A run of non-whitespace followed by whitespace (`\S+\s+`).
    #[default]
    Word,
    /// Up to and including a run of newlines (`\n+`).
    Line,
    /// The first match of a regular expression; the chunk spans from the
    /// buffer start to the end of the match. Empty matches fail the stream.
    Regex(Regex),
    /// The first Unicode word-boundary segment (UAX #29), emitted immediately.
    /// Whitespace and punctuation are separate segments; locale dictionaries
    /// and ICU tailoring require an application-supplied detector.
    UnicodeWords,
    /// A custom detector. Lengths of zero, beyond the buffer or inside a
    /// character fail the smoothing stream.
    Detector(ChunkDetector),
}

impl fmt::Debug for Chunking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Word => f.write_str("Word"),
            Self::Line => f.write_str("Line"),
            Self::Regex(regex) => f.debug_tuple("Regex").field(&regex.as_str()).finish(),
            Self::UnicodeWords => f.write_str("UnicodeWords"),
            Self::Detector(_) => f.write_str("Detector(..)"),
        }
    }
}

impl Chunking {
    /// Custom detector.
    #[must_use]
    pub fn detector(f: impl Fn(&str) -> Option<usize> + Send + Sync + 'static) -> Self {
        Self::Detector(Arc::new(f))
    }

    /// Byte length of the next chunk in `buffer`, if complete.
    #[must_use]
    pub fn detect(&self, buffer: &str) -> Option<usize> {
        self.detect_checked(buffer).ok().flatten()
    }

    fn detect_checked(&self, buffer: &str) -> Result<Option<usize>, Error> {
        let end = match self {
            Self::Word => word_chunk(buffer),
            Self::Line => line_chunk(buffer),
            Self::Regex(regex) => match regex.find(buffer) {
                Some(found) if found.is_empty() => {
                    return Err(Error::invalid_argument(
                        "chunking",
                        "chunking regex must not match an empty string",
                    ));
                }
                found => found.map(|found| found.end()),
            },
            Self::UnicodeWords => unicode_word_chunk(buffer),
            Self::Detector(detector) => detector(buffer),
        };
        if let Some(end) = end
            && (end == 0 || end > buffer.len() || !buffer.is_char_boundary(end))
        {
            return Err(Error::invalid_argument(
                "chunking",
                "chunk detector must return a nonempty UTF-8 prefix length",
            ));
        }
        Ok(end)
    }
}

/// End of the first `\S+\s+` match, measured from the buffer start.
fn word_chunk(buffer: &str) -> Option<usize> {
    let mut seen_word = false;
    let mut in_trailing_space = false;
    for (index, ch) in buffer.char_indices() {
        if !seen_word {
            if !ch.is_whitespace() {
                seen_word = true;
            }
        } else if ch.is_whitespace() {
            in_trailing_space = true;
        } else if in_trailing_space {
            return Some(index);
        }
    }
    in_trailing_space.then_some(buffer.len())
}

/// End of the first `\n+` match, measured from the buffer start.
fn line_chunk(buffer: &str) -> Option<usize> {
    let start = buffer.find('\n')?;
    let rest = &buffer[start..];
    let run = rest
        .char_indices()
        .find(|(_, ch)| *ch != '\n')
        .map_or(rest.len(), |(index, _)| index);
    Some(start + run)
}

/// The first segment, matching the reference Intl.Segmenter adapter's
/// emission timing without assuming locale-specific dictionary support.
fn unicode_word_chunk(buffer: &str) -> Option<usize> {
    buffer.split_word_bounds().next().map(str::len)
}

/// Configuration of [`smooth_stream`].
#[derive(Debug, Clone)]
pub struct SmoothStreamConfig {
    /// Pause after each emitted chunk (default 10 ms; `None` for no pause).
    pub delay: Option<Duration>,
    /// Chunking strategy (default [`Chunking::Word`]).
    pub chunking: Chunking,
}

impl Default for SmoothStreamConfig {
    fn default() -> Self {
        Self {
            delay: Some(Duration::from_millis(10)),
            chunking: Chunking::Word,
        }
    }
}

impl SmoothStreamConfig {
    /// The default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the pause after each chunk.
    #[must_use]
    pub fn delay(mut self, delay: Option<Duration>) -> Self {
        self.delay = delay;
        self
    }

    /// Sets the chunking strategy.
    #[must_use]
    pub fn chunking(mut self, chunking: Chunking) -> Self {
        self.chunking = chunking;
        self
    }
}

/// Creates the smoothing transform.
#[must_use]
pub fn smooth_stream(config: SmoothStreamConfig) -> SmoothStream {
    SmoothStream { config }
}

/// The smoothing transform; see [`smooth_stream`].
#[derive(Debug, Clone)]
pub struct SmoothStream {
    config: SmoothStreamConfig,
}

impl StreamTransform for SmoothStream {
    fn apply(&self, input: EventStream, ctx: TransformContext) -> EventStream {
        let state = SmoothState {
            input,
            delay: self.config.delay,
            chunking: self.config.chunking.clone(),
            buffer: String::new(),
            current: None,
            provider_metadata: None,
            pending: VecDeque::new(),
            delay_pending: false,
            done: false,
            context: ctx,
        };
        Box::pin(stream::unfold(state, |mut state| async move {
            loop {
                if let Some((event, delay_after)) = state.pending.pop_front() {
                    if state.delay_pending
                        && let Some(delay) = state.delay
                    {
                        let cancelled = tokio::select! {
                            biased;
                            () = state.context.cancellation().cancelled() => true,
                            () = async {
                                match tokio::time::Instant::now().checked_add(delay) {
                                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                                    None => std::future::pending().await,
                                }
                            } => false,
                        };
                        if cancelled {
                            state.pending.clear();
                            state.buffer.clear();
                            state.delay_pending = false;
                            continue;
                        }
                    }
                    state.delay_pending = delay_after;
                    return Some((event, state));
                }
                if state.done {
                    return None;
                }
                match state.input.next().await {
                    Some(event) => {
                        if let Err(error) = state.handle(event) {
                            let info = StreamErrorInfo::from_error(&error);
                            state.context.fail(error);
                            state.pending.clear();
                            state.buffer.clear();
                            state.delay_pending = false;
                            state.done = true;
                            state
                                .pending
                                .push_back((StreamEvent::Error { error: info }, false));
                        }
                    }
                    None => {
                        state.done = true;
                        state.flush();
                    }
                }
            }
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Text,
    Reasoning,
}

struct SmoothState {
    input: EventStream,
    delay: Option<Duration>,
    chunking: Chunking,
    buffer: String,
    current: Option<(Kind, PartId)>,
    provider_metadata: Option<ProviderMetadata>,
    pending: VecDeque<(StreamEvent, bool)>,
    delay_pending: bool,
    done: bool,
    context: TransformContext,
}

impl SmoothState {
    fn handle(&mut self, event: StreamEvent) -> Result<(), Error> {
        match event {
            StreamEvent::TextDelta {
                id,
                text,
                provider_metadata,
            } => self.smooth(Kind::Text, id, text, provider_metadata),
            StreamEvent::ReasoningDelta {
                id,
                text,
                provider_metadata,
            } => self.smooth(Kind::Reasoning, id, text, provider_metadata),
            other => {
                self.flush();
                self.pending.push_back((other, false));
                Ok(())
            }
        }
    }

    fn smooth(
        &mut self,
        kind: Kind,
        id: PartId,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Result<(), Error> {
        let same_part = self
            .current
            .as_ref()
            .is_some_and(|(current_kind, current_id)| *current_kind == kind && *current_id == id);
        if !same_part {
            self.flush();
        }
        self.buffer.push_str(&text);
        self.current = Some((kind, id));
        if provider_metadata.is_some() {
            self.provider_metadata = provider_metadata;
        }
        while let Some(end) = self.chunking.detect_checked(&self.buffer)? {
            let chunk: String = self.buffer.drain(..end).collect();
            if let Some(event) = self.delta(chunk, None) {
                self.pending.push_back((event, true));
            }
        }
        Ok(())
    }

    fn flush(&mut self) {
        if self.buffer.is_empty() && self.provider_metadata.is_none() {
            return;
        }
        let text = std::mem::take(&mut self.buffer);
        let metadata = self.provider_metadata.take();
        if let Some(event) = self.delta(text, metadata) {
            self.pending.push_back((event, false));
        }
    }

    fn delta(
        &self,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Option<StreamEvent> {
        let (kind, id) = self.current.clone()?;
        Some(match kind {
            Kind::Text => StreamEvent::TextDelta {
                id,
                text,
                provider_metadata,
            },
            Kind::Reasoning => StreamEvent::ReasoningDelta {
                id,
                text,
                provider_metadata,
            },
        })
    }
}
