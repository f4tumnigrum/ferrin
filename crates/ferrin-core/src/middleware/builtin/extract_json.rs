//! JSON extraction from fenced text.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::StreamResult;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::GenerateResult;
use futures_util::StreamExt;
use futures_util::stream;

use crate::middleware::GenerateNext;
use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;
use crate::middleware::StreamNext;

/// Rewrites the complete text of a text part.
pub type JsonTransformFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Middleware created by [`extract_json`].
#[derive(Clone)]
pub struct ExtractJson {
    transform: Option<JsonTransformFn>,
}

impl fmt::Debug for ExtractJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtractJson")
            .field("custom_transform", &self.transform.is_some())
            .finish()
    }
}

/// Strips a markdown code fence (```` ```json ```` .. ```` ``` ````) around
/// text parts and trims the result.
///
/// Streams are rewritten incrementally: the opening fence is removed once a
/// full line is available, the last twelve characters are held back until
/// `text-end` so the closing fence can be removed, and `text-start` is
/// delayed until the first delta is forwarded. A custom transform buffers
/// the whole part and applies at `text-end`.
#[must_use]
pub fn extract_json() -> ExtractJson {
    ExtractJson { transform: None }
}

/// Characters held back while streaming so a trailing fence can be removed.
const SUFFIX_BUFFER_CHARS: usize = 12;

impl ExtractJson {
    /// Replaces the default fence stripping with `transform`.
    #[must_use]
    pub fn transform(mut self, transform: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
        self.transform = Some(Arc::new(transform));
        self
    }

    fn run_transform(&self, text: &str) -> String {
        match &self.transform {
            Some(transform) => transform(text),
            None => strip_json_fences(text),
        }
    }

    /// Applies the extraction to a complete result.
    #[must_use]
    pub fn apply(&self, mut result: GenerateResult) -> GenerateResult {
        for part in &mut result.content {
            if let Content::Text { text, .. } = part {
                *text = self.run_transform(text);
            }
        }
        result
    }

    /// Applies the extraction to a stream result.
    #[must_use]
    pub fn apply_stream(&self, result: StreamResult) -> StreamResult {
        let StreamResult {
            stream,
            request,
            response,
        } = result;
        let mut state = StreamState {
            transform: self.transform.clone(),
            blocks: HashMap::new(),
        };
        let stream = stream
            .map(move |part| stream::iter(state.process(part)))
            .flatten();
        StreamResult {
            stream: Box::pin(stream),
            request,
            response,
        }
    }
}

impl LanguageModelMiddleware for ExtractJson {
    fn wrap_generate<'a>(
        &'a self,
        options: CallOptions,
        next: GenerateNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<GenerateResult, ProviderError>> {
        Box::pin(async move { Ok(self.apply(next(options).await?)) })
    }

    fn wrap_stream<'a>(
        &'a self,
        options: CallOptions,
        next: StreamNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<StreamResult, ProviderError>> {
        Box::pin(async move { Ok(self.apply_stream(next(options).await?)) })
    }
}

/// Default rewrite: strip the opening and closing fences, then trim.
#[must_use]
pub fn strip_json_fences(text: &str) -> String {
    strip_fence_suffix(strip_fence_prefix(text))
        .trim()
        .to_owned()
}

/// Removes ```` ``` ```` or ```` ```json ```` and the following whitespace.
fn strip_fence_prefix(text: &str) -> &str {
    match text.strip_prefix("```") {
        Some(rest) => rest.strip_prefix("json").unwrap_or(rest).trim_start(),
        None => text,
    }
}

/// Removes a trailing ```` ``` ```` (with trailing whitespace and one
/// preceding newline).
fn strip_fence_suffix(text: &str) -> &str {
    let trimmed = text.trim_end();
    match trimmed.strip_suffix("```") {
        Some(rest) => rest.strip_suffix('\n').unwrap_or(rest),
        None => text,
    }
}

fn strip_markdown_code_fence_suffix(text: &str) -> String {
    strip_fence_suffix(text).trim_end().to_owned()
}

/// Length of a complete opening fence line (```` ```json ```` plus
/// whitespace ending in a newline), when `text` starts with one.
fn fence_prefix_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("```")?;
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let consumed = text.len() - rest.len();
    let whitespace_len = rest
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map_or(rest.len(), |(index, _)| index);
    let whitespace = &rest[..whitespace_len];
    let last_newline = whitespace.rfind('\n')?;
    Some(consumed + last_newline + 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prefix,
    Streaming,
    Buffering,
}

struct Block {
    start: StreamPart,
    phase: Phase,
    buffer: String,
    prefix_stripped: bool,
}

struct StreamState {
    transform: Option<JsonTransformFn>,
    blocks: HashMap<PartId, Block>,
}

impl StreamState {
    fn process(&mut self, part: StreamPart) -> Vec<StreamPart> {
        let mut out = Vec::new();
        match part {
            StreamPart::TextStart { ref id, .. } => {
                let phase = if self.transform.is_some() {
                    Phase::Buffering
                } else {
                    Phase::Prefix
                };
                self.blocks.insert(
                    id.clone(),
                    Block {
                        start: part,
                        phase,
                        buffer: String::new(),
                        prefix_stripped: false,
                    },
                );
            }
            StreamPart::TextDelta { id, delta, .. } => {
                let Some(block) = self.blocks.get_mut(&id) else {
                    out.push(StreamPart::TextDelta {
                        id,
                        delta,
                        provider_metadata: None,
                    });
                    return out;
                };
                block.buffer.push_str(&delta);
                if block.phase == Phase::Buffering {
                    return out;
                }
                if block.phase == Phase::Prefix {
                    if !block.buffer.is_empty() && !block.buffer.starts_with('`') {
                        block.phase = Phase::Streaming;
                        out.push(block.start.clone());
                    } else if block.buffer.starts_with("```") {
                        // Strip the fence only once the line is complete.
                        if block.buffer.contains('\n') {
                            if let Some(len) = fence_prefix_len(&block.buffer) {
                                block.buffer = block.buffer[len..].to_owned();
                                block.prefix_stripped = true;
                            }
                            block.phase = Phase::Streaming;
                            out.push(block.start.clone());
                        }
                    } else if block.buffer.chars().count() >= 3 {
                        block.phase = Phase::Streaming;
                        out.push(block.start.clone());
                    }
                }
                if block.phase == Phase::Streaming {
                    let count = block.buffer.chars().count();
                    if count > SUFFIX_BUFFER_CHARS {
                        let split = block
                            .buffer
                            .char_indices()
                            .nth(count - SUFFIX_BUFFER_CHARS)
                            .map_or(block.buffer.len(), |(index, _)| index);
                        let to_stream = block.buffer[..split].to_owned();
                        block.buffer = block.buffer[split..].to_owned();
                        out.push(StreamPart::TextDelta {
                            id,
                            delta: to_stream,
                            provider_metadata: None,
                        });
                    }
                }
            }
            StreamPart::TextEnd { ref id, .. } => {
                if let Some(block) = self.blocks.remove(id) {
                    if matches!(block.phase, Phase::Prefix | Phase::Buffering) {
                        out.push(block.start);
                    }
                    let remaining = match block.phase {
                        Phase::Buffering => match &self.transform {
                            Some(transform) => transform(&block.buffer),
                            None => strip_json_fences(&block.buffer),
                        },
                        // Nothing streamed yet: the full transform is safe.
                        Phase::Prefix => strip_json_fences(&block.buffer),
                        // Earlier text already streamed: only strip the
                        // suffix so leading whitespace at the boundary stays.
                        Phase::Streaming => strip_markdown_code_fence_suffix(&block.buffer),
                    };
                    if !remaining.is_empty() {
                        out.push(StreamPart::TextDelta {
                            id: id.clone(),
                            delta: remaining,
                            provider_metadata: None,
                        });
                    }
                }
                out.push(part);
            }
            other => out.push(other),
        }
        out
    }
}
