//! Buffered and streaming outgoing request bodies.

use std::fmt;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::stream;

use super::multipart::MultipartForm;
use super::transport::BodyStream;
use super::transport::TransportError;
use super::transport::TransportErrorKind;

/// Body of an outgoing request.
#[non_exhaustive]
pub enum RequestBody {
    /// No body.
    Empty,
    /// Raw bytes with a content type.
    Bytes {
        /// `Content-Type` value.
        content_type: String,
        /// Payload.
        data: Bytes,
    },
    /// A multipart form, potentially containing streaming files.
    Multipart(MultipartForm),
    /// A one-shot body stream.
    Stream {
        /// `Content-Type` value.
        content_type: String,
        /// Payload chunks, consumed only when the transport sends them.
        data: BodyStream,
        /// Exact byte length when known; mismatches fail the upload.
        content_length: Option<u64>,
    },
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Bytes { content_type, data } => f
                .debug_struct("Bytes")
                .field("content_type", content_type)
                .field("byte_length", &data.len())
                .finish(),
            Self::Multipart(form) => f.debug_tuple("Multipart").field(form).finish(),
            Self::Stream {
                content_type,
                content_length,
                ..
            } => f
                .debug_struct("Stream")
                .field("content_type", content_type)
                .field("content_length", content_length)
                .finish_non_exhaustive(),
        }
    }
}

impl RequestBody {
    /// A JSON body.
    #[must_use]
    pub fn json(data: Bytes) -> Self {
        Self::Bytes {
            content_type: "application/json".to_owned(),
            data,
        }
    }

    /// Returns the `Content-Type` this body requires, if any.
    #[must_use]
    pub fn content_type(&self) -> Option<String> {
        match self {
            Self::Empty => None,
            Self::Bytes { content_type, .. } | Self::Stream { content_type, .. } => {
                Some(content_type.clone())
            }
            Self::Multipart(form) => Some(form.content_type()),
        }
    }

    /// The encoded byte length, when all part lengths are known.
    #[must_use]
    pub fn content_length(&self) -> Option<u64> {
        match self {
            Self::Empty => Some(0),
            Self::Bytes { data, .. } => Some(data.len() as u64),
            Self::Multipart(form) => form.content_length(),
            Self::Stream { content_length, .. } => *content_length,
        }
    }

    /// Encodes a buffered body synchronously.
    ///
    /// # Errors
    ///
    /// Returns [`TransportErrorKind::InvalidRequest`] for streaming bodies.
    /// Use [`Self::into_stream`] to consume those bodies incrementally.
    pub fn to_bytes(&self) -> Result<Bytes, TransportError> {
        match self {
            Self::Empty => Ok(Bytes::new()),
            Self::Bytes { data, .. } => Ok(data.clone()),
            Self::Multipart(form) => form.encode(),
            Self::Stream { .. } => Err(streaming_body_error()),
        }
    }

    /// Consumes the request as a stream, without collecting file contents.
    #[must_use]
    pub fn into_stream(self) -> BodyStream {
        match self {
            Self::Empty => Box::pin(stream::empty()),
            Self::Bytes { data, .. } => Box::pin(stream::once(std::future::ready(Ok(data)))),
            Self::Multipart(form) => form.into_stream(),
            Self::Stream {
                data,
                content_length,
                ..
            } => checked_stream(data, content_length),
        }
    }
}

pub(super) fn streaming_body_error() -> TransportError {
    TransportError::new(
        TransportErrorKind::InvalidRequest,
        "streaming request bodies must be consumed asynchronously",
    )
}

pub(super) fn checked_stream(inner: BodyStream, length: Option<u64>) -> BodyStream {
    Box::pin(CheckedStream {
        inner: Some(inner),
        remaining: length,
    })
}

struct CheckedStream {
    inner: Option<BodyStream>,
    remaining: Option<u64>,
}

impl Stream for CheckedStream {
    type Item = Result<Bytes, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Some(inner) = &mut self.inner else {
            return Poll::Ready(None);
        };
        match inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if let Some(remaining) = &mut self.remaining {
                    if chunk.len() as u64 > *remaining {
                        self.inner = None;
                        return Poll::Ready(Some(Err(length_error())));
                    }
                    *remaining -= chunk.len() as u64;
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.inner = None;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.inner = None;
                if self.remaining.is_some_and(|remaining| remaining != 0) {
                    Poll::Ready(Some(Err(length_error())))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn length_error() -> TransportError {
    TransportError::new(
        TransportErrorKind::Body,
        "upload body does not match its declared length",
    )
}
