//! `smooth_stream`: buffers text and reasoning deltas and re-emits them in
//! word- or line-sized chunks with a small delay, producing an even typing
//! rhythm regardless of provider chunking.

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
use crate::stream_text::EventStream;
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
    /// buffer start to the end of the match. Empty matches are ignored.
    Regex(Regex),
    /// Unicode word boundaries (UAX #29): one word plus the whitespace that
    /// follows it. Splits scripts written without spaces character by
    /// character.
    UnicodeWords,
    /// A custom detector. Lengths of zero, beyond the buffer or inside a
    /// character are ignored.
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
        let end = match self {
            Self::Word => word_chunk(buffer),
            Self::Line => line_chunk(buffer),
            Self::Regex(regex) => regex.find(buffer).map(|found| found.end()),
            Self::UnicodeWords => unicode_word_chunk(buffer),
            Self::Detector(detector) => detector(buffer),
        }?;
        (end > 0 && end <= buffer.len() && buffer.is_char_boundary(end)).then_some(end)
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

/// First word (after leading whitespace) plus the whitespace following it,
/// once the next word has started or the buffer ends in whitespace.
fn unicode_word_chunk(buffer: &str) -> Option<usize> {
    let mut seen_word = false;
    let mut in_trailing_space = false;
    for (index, segment) in buffer.split_word_bound_indices() {
        let is_space = segment.chars().all(char::is_whitespace);
        if !seen_word {
            if !is_space {
                seen_word = true;
            }
        } else if is_space {
            in_trailing_space = true;
        } else {
            return Some(index);
        }
    }
    in_trailing_space.then_some(buffer.len())
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
    fn apply(&self, input: EventStream, _ctx: TransformContext) -> EventStream {
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
        };
        Box::pin(stream::unfold(state, |mut state| async move {
            loop {
                if let Some((event, delay_after)) = state.pending.pop_front() {
                    if state.delay_pending
                        && let Some(delay) = state.delay
                    {
                        tokio::time::sleep(delay).await;
                    }
                    state.delay_pending = delay_after;
                    return Some((event, state));
                }
                if state.done {
                    return None;
                }
                match state.input.next().await {
                    Some(event) => state.handle(event),
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
}

impl SmoothState {
    fn handle(&mut self, event: StreamEvent) {
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
            }
        }
    }

    fn smooth(
        &mut self,
        kind: Kind,
        id: PartId,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    ) {
        let same_part = self
            .current
            .as_ref()
            .is_some_and(|(current_kind, current_id)| *current_kind == kind && *current_id == id);
        if !same_part || provider_metadata.is_some() {
            self.flush();
            self.provider_metadata = provider_metadata;
        }
        self.buffer.push_str(&text);
        self.current = Some((kind, id));
        if text.is_empty() && self.provider_metadata.is_some() {
            self.flush();
        }
        while let Some(end) = self.chunking.detect(&self.buffer) {
            let chunk: String = self.buffer.drain(..end).collect();
            let metadata = self.provider_metadata.take();
            if let Some(event) = self.delta(chunk, metadata) {
                self.pending.push_back((event, true));
            }
        }
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
