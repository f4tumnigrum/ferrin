//! Reasoning extraction from tagged text.

use std::collections::HashMap;

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
use regex::Regex;

use crate::middleware::GenerateNext;
use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;
use crate::middleware::StreamNext;

/// Middleware created by [`extract_reasoning`].
#[derive(Debug, Clone)]
pub struct ExtractReasoning {
    opening_tag: String,
    closing_tag: String,
    separator: String,
    start_with_reasoning: bool,
    pattern: Regex,
}

/// Extracts `<tag_name>..</tag_name>` sections from text parts into
/// reasoning parts.
///
/// Complete responses: every match becomes reasoning (joined by the
/// separator, default `"\n"`) placed before the remaining text. Streams:
/// deltas are split at tag boundaries into `reasoning-*` parts with ids
/// `reasoning-<n>` and the original text part; `text-start` is delayed
/// until the first text delta so it never precedes `reasoning-start`, and
/// empty sections still emit `reasoning-start`/`reasoning-end`.
///
/// # Panics
///
/// Never: the tag name is escaped before being compiled into a pattern.
#[must_use]
pub fn extract_reasoning(tag_name: impl AsRef<str>) -> ExtractReasoning {
    let tag_name = tag_name.as_ref();
    let opening_tag = format!("<{tag_name}>");
    let closing_tag = format!("</{tag_name}>");
    let source = format!(
        "(?s){}(.*?){}",
        regex::escape(&opening_tag),
        regex::escape(&closing_tag)
    );
    let pattern = match Regex::new(&source) {
        Ok(pattern) => pattern,
        Err(_) => unreachable!("escaped tags form a valid pattern"),
    };
    ExtractReasoning {
        opening_tag,
        closing_tag,
        separator: "\n".to_owned(),
        start_with_reasoning: false,
        pattern,
    }
}

impl ExtractReasoning {
    /// Separator between extracted sections (default `"\n"`).
    #[must_use]
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Treats the text as starting inside a tag (for models that omit the
    /// opening tag).
    #[must_use]
    pub fn start_with_reasoning(mut self, start_with_reasoning: bool) -> Self {
        self.start_with_reasoning = start_with_reasoning;
        self
    }

    /// Applies the extraction to a complete result.
    #[must_use]
    pub fn apply(&self, mut result: GenerateResult) -> GenerateResult {
        let mut transformed = Vec::with_capacity(result.content.len() + 1);
        for part in result.content.drain(..) {
            let Content::Text {
                text,
                provider_metadata,
            } = part
            else {
                transformed.push(part);
                continue;
            };
            let subject = if self.start_with_reasoning {
                format!("{}{text}", self.opening_tag)
            } else {
                text.clone()
            };
            let matches: Vec<(std::ops::Range<usize>, String)> = self
                .pattern
                .captures_iter(&subject)
                .filter_map(|captures| {
                    let whole = captures.get(0)?;
                    let inner = captures.get(1)?;
                    Some((whole.range(), inner.as_str().to_owned()))
                })
                .collect();
            if matches.is_empty() {
                transformed.push(Content::Text {
                    text,
                    provider_metadata,
                });
                continue;
            }
            let reasoning = matches
                .iter()
                .map(|(_, inner)| inner.as_str())
                .collect::<Vec<_>>()
                .join(&self.separator);
            let mut remaining = subject;
            for (range, _) in matches.iter().rev() {
                let before = &remaining[..range.start];
                let after = &remaining[range.end..];
                let separator = if before.is_empty() || after.is_empty() {
                    ""
                } else {
                    self.separator.as_str()
                };
                remaining = format!("{before}{separator}{after}");
            }
            transformed.push(Content::Reasoning {
                text: reasoning,
                provider_metadata: None,
            });
            transformed.push(Content::Text {
                text: remaining,
                provider_metadata,
            });
        }
        result.content = transformed;
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
            opening_tag: self.opening_tag.clone(),
            closing_tag: self.closing_tag.clone(),
            separator: self.separator.clone(),
            start_with_reasoning: self.start_with_reasoning,
            extractions: HashMap::new(),
            delayed_text_start: None,
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

impl LanguageModelMiddleware for ExtractReasoning {
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

struct Extraction {
    is_first_reasoning: bool,
    is_first_text: bool,
    after_switch: bool,
    is_reasoning: bool,
    buffer: String,
    id_counter: u32,
    text_id: PartId,
}

struct StreamState {
    opening_tag: String,
    closing_tag: String,
    separator: String,
    start_with_reasoning: bool,
    extractions: HashMap<PartId, Extraction>,
    delayed_text_start: Option<StreamPart>,
}

fn reasoning_id(counter: u32) -> PartId {
    PartId::new(format!("reasoning-{counter}"))
}

impl StreamState {
    fn process(&mut self, part: StreamPart) -> Vec<StreamPart> {
        let mut out = Vec::new();
        match part {
            // Never send `text-start` before `reasoning-start`.
            StreamPart::TextStart { .. } => {
                self.delayed_text_start = Some(part);
            }
            StreamPart::TextEnd { .. } => {
                out.extend(self.delayed_text_start.take());
                out.push(part);
            }
            StreamPart::TextDelta { id, delta, .. } => {
                self.process_delta(&id, &delta, &mut out);
            }
            other => out.push(other),
        }
        out
    }

    fn process_delta(&mut self, id: &PartId, delta: &str, out: &mut Vec<StreamPart>) {
        let start_with_reasoning = self.start_with_reasoning;
        let extraction = self
            .extractions
            .entry(id.clone())
            .or_insert_with(|| Extraction {
                is_first_reasoning: true,
                is_first_text: true,
                after_switch: false,
                is_reasoning: start_with_reasoning,
                buffer: String::new(),
                id_counter: 0,
                text_id: id.clone(),
            });
        extraction.buffer.push_str(delta);
        loop {
            let next_tag = if extraction.is_reasoning {
                self.closing_tag.as_str()
            } else {
                self.opening_tag.as_str()
            };
            let Some(start) = potential_start_index(&extraction.buffer, next_tag) else {
                let buffer = std::mem::take(&mut extraction.buffer);
                publish(
                    extraction,
                    &buffer,
                    &self.separator,
                    &mut self.delayed_text_start,
                    out,
                );
                break;
            };
            let before = extraction.buffer[..start].to_owned();
            publish(
                extraction,
                &before,
                &self.separator,
                &mut self.delayed_text_start,
                out,
            );
            let end = start + next_tag.len();
            if end <= extraction.buffer.len() {
                extraction.buffer = extraction.buffer[end..].to_owned();
                if extraction.is_reasoning {
                    // Empty sections still open a reasoning part.
                    if extraction.is_first_reasoning {
                        out.push(StreamPart::ReasoningStart {
                            id: reasoning_id(extraction.id_counter),
                            provider_metadata: None,
                        });
                    }
                    out.push(StreamPart::ReasoningEnd {
                        id: reasoning_id(extraction.id_counter),
                        provider_metadata: None,
                    });
                    extraction.id_counter += 1;
                }
                extraction.is_reasoning = !extraction.is_reasoning;
                extraction.after_switch = true;
            } else {
                extraction.buffer = extraction.buffer[start..].to_owned();
                break;
            }
        }
    }
}

fn publish(
    extraction: &mut Extraction,
    text: &str,
    separator: &str,
    delayed_text_start: &mut Option<StreamPart>,
    out: &mut Vec<StreamPart>,
) {
    if text.is_empty() {
        return;
    }
    let continuing = if extraction.is_reasoning {
        !extraction.is_first_reasoning
    } else {
        !extraction.is_first_text
    };
    let prefix = if extraction.after_switch && continuing {
        separator
    } else {
        ""
    };
    if extraction.is_reasoning {
        if extraction.after_switch || extraction.is_first_reasoning {
            out.push(StreamPart::ReasoningStart {
                id: reasoning_id(extraction.id_counter),
                provider_metadata: None,
            });
        }
        out.push(StreamPart::ReasoningDelta {
            id: reasoning_id(extraction.id_counter),
            delta: format!("{prefix}{text}"),
            provider_metadata: None,
        });
        extraction.is_first_reasoning = false;
    } else {
        out.extend(delayed_text_start.take());
        out.push(StreamPart::TextDelta {
            id: extraction.text_id.clone(),
            delta: format!("{prefix}{text}"),
            provider_metadata: None,
        });
        extraction.is_first_text = false;
    }
    extraction.after_switch = false;
}

/// Index of `needle` in `text`, or of the longest suffix of `text` that is
/// a prefix of `needle` (a tag possibly split across deltas).
fn potential_start_index(text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    if let Some(index) = text.find(needle) {
        return Some(index);
    }
    text.char_indices()
        .rev()
        .find(|(index, _)| needle.starts_with(&text[*index..]))
        .map(|(index, _)| index)
}
