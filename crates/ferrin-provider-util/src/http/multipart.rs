//! Incremental multipart encoding for buffered and streamed files.

use std::fmt;

use bytes::Bytes;
use ferrin_spec::JsonValue;
use futures_util::StreamExt;
use futures_util::stream;
use serde_json::json;

use super::request_body::checked_stream;
use super::request_body::streaming_body_error;
use super::transport::BodyStream;
use super::transport::TransportError;

/// A `multipart/form-data` body with buffered or streaming files.
#[derive(Debug)]
pub struct MultipartForm {
    parts: Vec<MultipartPart>,
    boundary: String,
}

/// One part of a [`MultipartForm`].
#[non_exhaustive]
pub enum MultipartPart {
    /// A text field.
    Field {
        /// Field name.
        name: String,
        /// Field value.
        value: String,
    },
    /// An in-memory file.
    File {
        /// Field name.
        name: String,
        /// File name sent in the disposition (`blob` when absent).
        filename: Option<String>,
        /// Media type (`application/octet-stream` when absent).
        media_type: Option<String>,
        /// File bytes.
        data: Bytes,
    },
    /// A file whose contents are sent as they become available.
    StreamFile {
        /// Field name.
        name: String,
        /// File name sent in the disposition (`blob` when absent).
        filename: Option<String>,
        /// Media type (`application/octet-stream` when absent).
        media_type: Option<String>,
        /// File chunks, consumed once by the transport.
        data: BodyStream,
        /// Exact file byte length, if known; mismatches fail the upload.
        content_length: Option<u64>,
    },
}

impl fmt::Debug for MultipartPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct(match self {
            Self::Field { .. } => "Field",
            Self::File { .. } => "File",
            Self::StreamFile { .. } => "StreamFile",
        });
        match self {
            Self::Field { name, value } => {
                debug.field("name", name).field("byte_length", &value.len());
            }
            Self::File {
                name,
                filename,
                media_type,
                data,
            } => {
                debug
                    .field("name", name)
                    .field("filename", filename)
                    .field("media_type", media_type)
                    .field("byte_length", &data.len());
            }
            Self::StreamFile {
                name,
                filename,
                media_type,
                content_length,
                ..
            } => {
                debug
                    .field("name", name)
                    .field("filename", filename)
                    .field("media_type", media_type)
                    .field("content_length", content_length);
            }
        }
        debug.finish_non_exhaustive()
    }
}

impl Default for MultipartForm {
    fn default() -> Self {
        Self::new()
    }
}

impl MultipartForm {
    /// Creates an empty form with a random boundary.
    #[must_use]
    pub fn new() -> Self {
        Self::with_boundary(format!("ferrin-multipart-{}", crate::ids::generate_id()))
    }

    /// Creates an empty form with a fixed boundary (for tests and snapshots).
    #[must_use]
    pub fn with_boundary(boundary: impl Into<String>) -> Self {
        Self {
            parts: Vec::new(),
            boundary: boundary.into(),
        }
    }

    /// Appends a part, including a stream with a declared length.
    #[must_use]
    pub fn part(mut self, part: MultipartPart) -> Self {
        self.parts.push(part);
        self
    }

    /// Adds a text field.
    #[must_use]
    pub fn field(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.part(MultipartPart::Field {
            name: name.into(),
            value: value.into(),
        })
    }

    /// Adds an in-memory file.
    #[must_use]
    pub fn file(
        self,
        name: impl Into<String>,
        filename: Option<String>,
        media_type: Option<String>,
        data: Bytes,
    ) -> Self {
        self.part(MultipartPart::File {
            name: name.into(),
            filename,
            media_type,
            data,
        })
    }

    /// Adds a file stream of unknown length without polling or collecting it.
    #[must_use]
    pub fn file_stream(
        self,
        name: impl Into<String>,
        filename: Option<String>,
        media_type: Option<String>,
        data: BodyStream,
    ) -> Self {
        self.part(MultipartPart::StreamFile {
            name: name.into(),
            filename,
            media_type,
            data,
            content_length: None,
        })
    }

    /// The parts, in transmission order.
    #[must_use]
    pub fn parts(&self) -> &[MultipartPart] {
        &self.parts
    }

    /// The boundary.
    #[must_use]
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    /// The `Content-Type` header value.
    #[must_use]
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    /// The total encoded length, if every file length is known.
    #[must_use]
    pub fn content_length(&self) -> Option<u64> {
        let mut total = self.ending().len() as u64;
        for part in &self.parts {
            let length = match part {
                MultipartPart::Field { value, .. } => value.len() as u64,
                MultipartPart::File { data, .. } => data.len() as u64,
                MultipartPart::StreamFile { content_length, .. } => (*content_length)?,
            };
            total = total.checked_add(part_header(&self.boundary, part).len() as u64)?;
            total = total.checked_add(length)?.checked_add(2)?;
        }
        Some(total)
    }

    /// A JSON summary of fields, with files shown as `<file:name>`.
    #[must_use]
    pub fn values(&self) -> JsonValue {
        let mut map = serde_json::Map::new();
        for part in &self.parts {
            match part {
                MultipartPart::Field { name, value } => {
                    map.insert(name.clone(), json!(value));
                }
                MultipartPart::File { name, filename, .. }
                | MultipartPart::StreamFile { name, filename, .. } => {
                    let label = filename.as_deref().unwrap_or(name);
                    map.insert(name.clone(), json!(format!("<file:{label}>")));
                }
            }
        }
        JsonValue::Object(map)
    }

    /// Encodes an entirely buffered form synchronously.
    ///
    /// # Errors
    ///
    /// Returns an invalid-request error when the form includes a streaming file.
    /// Use [`Self::into_stream`] for incremental encoding.
    pub fn encode(&self) -> Result<Bytes, TransportError> {
        if self
            .parts
            .iter()
            .any(|part| matches!(part, MultipartPart::StreamFile { .. }))
        {
            return Err(streaming_body_error());
        }
        let mut out = Vec::new();
        for part in &self.parts {
            out.extend_from_slice(&part_header(&self.boundary, part));
            match part {
                MultipartPart::Field { value, .. } => out.extend_from_slice(value.as_bytes()),
                MultipartPart::File { data, .. } => out.extend_from_slice(data),
                MultipartPart::StreamFile { .. } => return Err(streaming_body_error()),
            }
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(&self.ending());
        Ok(Bytes::from(out))
    }

    /// Emits headers, each file's chunks and delimiters without buffering files.
    #[must_use]
    pub fn into_stream(self) -> BodyStream {
        let ending = self.ending();
        let mut chunks = Vec::new();
        for part in self.parts {
            chunks.push(single_chunk(part_header(&self.boundary, &part)));
            chunks.push(match part {
                MultipartPart::Field { value, .. } => single_chunk(Bytes::from(value)),
                MultipartPart::File { data, .. } => single_chunk(data),
                MultipartPart::StreamFile {
                    data,
                    content_length,
                    ..
                } => checked_stream(data, content_length),
            });
            chunks.push(single_chunk(Bytes::from_static(b"\r\n")));
        }
        chunks.push(single_chunk(ending));
        checked_stream(Box::pin(stream::iter(chunks).flatten()), None)
    }

    fn ending(&self) -> Bytes {
        Bytes::from(format!("--{}--\r\n", self.boundary))
    }
}

fn single_chunk(chunk: Bytes) -> BodyStream {
    Box::pin(stream::once(std::future::ready(Ok(chunk))))
}

fn part_header(boundary: &str, part: &MultipartPart) -> Bytes {
    let (name, file) = match part {
        MultipartPart::Field { name, .. } => (name, None),
        MultipartPart::File {
            name,
            filename,
            media_type,
            ..
        }
        | MultipartPart::StreamFile {
            name,
            filename,
            media_type,
            ..
        } => (name, Some((filename, media_type))),
    };
    let mut out = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
        escape_header_value(name)
    );
    if let Some((filename, media_type)) = file {
        out.push_str(&format!(
            "; filename=\"{}\"\r\nContent-Type: ",
            escape_header_value(filename.as_deref().unwrap_or("blob"))
        ));
        out.extend(
            media_type
                .as_deref()
                .unwrap_or("application/octet-stream")
                .chars()
                .filter(|ch| *ch != '\r' && *ch != '\n'),
        );
    }
    out.push_str("\r\n\r\n");
    Bytes::from(out)
}

fn escape_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .flat_map(|ch| match ch {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            other => vec![other],
        })
        .collect()
}
