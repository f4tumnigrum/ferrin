//! Simulated provider streams.

use std::time::Duration;

use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::language_model::StreamResult;
use futures_util::stream;

/// Builds a [`StreamResult`] that yields `parts` in order without delay.
#[must_use]
pub fn simulate_stream(parts: impl IntoIterator<Item = StreamPart>) -> StreamResult {
    SimulatedStream::new(parts).build()
}

/// A scripted provider stream with optional delays, for timeout tests
/// (combine with `tokio::time::pause()`).
#[derive(Debug, Clone)]
pub struct SimulatedStream {
    parts: Vec<StreamPart>,
    initial_delay: Option<Duration>,
    chunk_delay: Option<Duration>,
    hang_at_end: bool,
}

impl SimulatedStream {
    /// Scripts `parts`.
    #[must_use]
    pub fn new(parts: impl IntoIterator<Item = StreamPart>) -> Self {
        Self {
            parts: parts.into_iter().collect(),
            initial_delay: None,
            chunk_delay: None,
            hang_at_end: false,
        }
    }

    /// Waits before the first part.
    #[must_use]
    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = Some(delay);
        self
    }

    /// Waits between consecutive parts.
    #[must_use]
    pub fn chunk_delay(mut self, delay: Duration) -> Self {
        self.chunk_delay = Some(delay);
        self
    }

    /// Never ends the stream after the last part (the consumer must cancel).
    #[must_use]
    pub fn hang_at_end(mut self) -> Self {
        self.hang_at_end = true;
        self
    }

    /// Builds the stream result.
    #[must_use]
    pub fn build(self) -> StreamResult {
        let Self {
            parts,
            initial_delay,
            chunk_delay,
            hang_at_end,
        } = self;
        let total = parts.len();
        let stream = stream::unfold(
            (parts.into_iter(), 0usize),
            move |(mut parts, index)| async move {
                match parts.next() {
                    Some(part) => {
                        let delay = if index == 0 {
                            initial_delay
                        } else {
                            chunk_delay
                        };
                        if let Some(delay) = delay {
                            tokio::time::sleep(delay).await;
                        }
                        Some((part, (parts, index + 1)))
                    }
                    None => {
                        if hang_at_end && index >= total {
                            std::future::pending::<()>().await;
                        }
                        None
                    }
                }
            },
        );
        StreamResult::new(Box::pin(stream))
    }
}

/// Parts of a complete text response: stream start, one text part made of
/// `deltas`, and a `stop` finish with `usage`.
#[must_use]
pub fn text_parts(
    deltas: impl IntoIterator<Item = impl Into<String>>,
    usage: Usage,
) -> Vec<StreamPart> {
    let id = PartId::new("0");
    let mut parts = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: id.clone(),
            provider_metadata: None,
        },
    ];
    parts.extend(
        deltas
            .into_iter()
            .map(|delta| StreamPart::text_delta(id.clone(), delta)),
    );
    parts.push(StreamPart::TextEnd {
        id,
        provider_metadata: None,
    });
    parts.push(StreamPart::finish(FinishReason::stop(), usage));
    parts
}
