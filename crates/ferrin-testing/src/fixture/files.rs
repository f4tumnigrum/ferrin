//! Fixture descriptions and file loading.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::Duration;

use bytes::Bytes;
use ferrin_spec::Headers;
use http::StatusCode;
use serde::Deserialize;
use serde_json::Value as JsonValue;

/// Body of a [`Fixture`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FixtureBody {
    /// Sent in one piece.
    Complete(Bytes),
    /// Server-sent events, one frame per event, sent with optional delays.
    Events {
        /// Encoded events (without the trailing blank line).
        events: Vec<String>,
        /// Delay before the first event.
        initial_delay: Option<Duration>,
        /// Delay between events; the server default applies when `None`.
        chunk_delay: Option<Duration>,
        /// Keep the response open after the last event until the client
        /// disconnects or the server stops (long-lived event streams).
        hold_open: bool,
    },
}

/// A canned HTTP response.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// Status code.
    pub status: StatusCode,
    /// Response headers (`content-type` is set by the constructors).
    pub headers: Headers,
    /// Body.
    pub body: FixtureBody,
}

impl Fixture {
    /// A `200` JSON response.
    #[must_use]
    pub fn json(value: &JsonValue) -> Self {
        Self::json_status(StatusCode::OK, value)
    }

    /// A JSON response with `status`.
    #[must_use]
    pub fn json_status(status: StatusCode, value: &JsonValue) -> Self {
        Self::complete(status, "application/json", Bytes::from(value.to_string()))
    }

    /// A complete response with the given content type.
    #[must_use]
    pub fn complete(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            headers: Headers::new().with("content-type", content_type),
            body: FixtureBody::Complete(body.into()),
        }
    }

    /// A `200 text/event-stream` response sending `events` one frame each.
    #[must_use]
    pub fn sse<I, S>(events: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            status: StatusCode::OK,
            headers: Headers::new()
                .with("content-type", "text/event-stream")
                .with("cache-control", "no-cache"),
            body: FixtureBody::Events {
                events: events.into_iter().map(Into::into).collect(),
                initial_delay: None,
                chunk_delay: None,
                hold_open: false,
            },
        }
    }

    /// A `200 text/event-stream` response whose events are `data: <json>`
    /// lines.
    #[must_use]
    pub fn sse_json<'a>(values: impl IntoIterator<Item = &'a JsonValue>) -> Self {
        Self::sse(values.into_iter().map(|value| format!("data: {value}")))
    }

    /// Overrides the status.
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Adds or replaces a header.
    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers = self.headers.with(name, value);
        self
    }

    /// Sets the delay between events (event fixtures only).
    #[must_use]
    pub fn with_chunk_delay(mut self, delay: Duration) -> Self {
        if let FixtureBody::Events { chunk_delay, .. } = &mut self.body {
            *chunk_delay = Some(delay);
        }
        self
    }

    /// Keeps the stream open after the last event (event fixtures only).
    #[must_use]
    pub fn hold_open(mut self) -> Self {
        if let FixtureBody::Events { hold_open, .. } = &mut self.body {
            *hold_open = true;
        }
        self
    }

    /// Sets the delay before the first event (event fixtures only).
    #[must_use]
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        if let FixtureBody::Events { initial_delay, .. } = &mut self.body {
            *initial_delay = Some(delay);
        }
        self
    }

    /// Loads `<dir>/<case>.response.json` or `<dir>/<case>.chunks.txt`,
    /// applying `<dir>/<case>.meta.json` when present.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` when neither body file exists, `InvalidData` when
    /// the meta file is malformed, and the underlying I/O error otherwise.
    pub fn load(dir: impl AsRef<Path>, case: &str) -> io::Result<Self> {
        let dir = dir.as_ref();
        let response_path = dir.join(format!("{case}.response.json"));
        let chunks_path = dir.join(format!("{case}.chunks.txt"));
        let mut fixture = if response_path.is_file() {
            let body = std::fs::read(&response_path)?;
            Self::complete(StatusCode::OK, "application/json", body)
        } else if chunks_path.is_file() {
            let text = std::fs::read_to_string(&chunks_path)?;
            Self::sse(decode_events_file(&text))
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "no fixture body for `{case}` in {} (expected .response.json or .chunks.txt)",
                    dir.display()
                ),
            ));
        };
        let meta_path = dir.join(format!("{case}.meta.json"));
        if meta_path.is_file() {
            let text = std::fs::read_to_string(&meta_path)?;
            let meta: FixtureMeta = serde_json::from_str(&text).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid meta file {}: {error}", meta_path.display()),
                )
            })?;
            if let Some(status) = meta.status {
                fixture.status = StatusCode::from_u16(status).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid status in {}: {error}", meta_path.display()),
                    )
                })?;
            }
            for (name, value) in &meta.headers {
                fixture.headers.insert(name, value).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "invalid header `{name}` in {}: {error}",
                            meta_path.display()
                        ),
                    )
                })?;
            }
        }
        Ok(fixture)
    }
}

#[derive(Debug, Default, Deserialize)]
struct FixtureMeta {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

/// Decodes a `.chunks.txt` file: one event per line, `\n` for newlines
/// inside an event and `\\` for a backslash. Empty lines are skipped.
#[must_use]
pub fn decode_events_file(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(unescape_line)
        .collect()
}

/// Encodes events for a `.chunks.txt` file (inverse of
/// [`decode_events_file`]); the result ends with a newline.
#[must_use]
pub fn encode_events_file<'a>(events: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for event in events {
        for c in event.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                other => out.push(other),
            }
        }
        out.push('\n');
    }
    out
}

fn unescape_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
