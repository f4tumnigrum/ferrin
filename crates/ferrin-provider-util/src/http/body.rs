//! Size-limited body reading.

use bytes::Bytes;
use bytes::BytesMut;
use ferrin_spec::Headers;
use futures_util::StreamExt;

use super::transport::BodyStream;
use super::transport::TransportError;
use super::transport::TransportErrorKind;

/// Default response body limit (2 GiB).
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Reads a body stream into memory, failing once `max_bytes` is exceeded.
///
/// A `Content-Length` header above the limit fails before reading.
///
/// # Errors
///
/// Returns the stream's error, or a [`TransportErrorKind::BodyTooLarge`]
/// error when the limit is exceeded.
pub async fn read_body(
    headers: &Headers,
    mut body: BodyStream,
    max_bytes: u64,
) -> Result<Bytes, TransportError> {
    if let Some(length) = headers
        .get_str("content-length")
        .and_then(|value| value.trim().parse::<u64>().ok())
        && length > max_bytes
    {
        return Err(too_large(max_bytes, Some(length)));
    }
    let mut buffer = BytesMut::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        let total = buffer.len() as u64 + chunk.len() as u64;
        if total > max_bytes {
            return Err(too_large(max_bytes, None));
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(buffer.freeze())
}

fn too_large(max_bytes: u64, declared: Option<u64>) -> TransportError {
    let message = match declared {
        Some(length) => {
            format!("response body of {length} bytes exceeds the limit of {max_bytes} bytes")
        }
        None => format!("response body exceeds the limit of {max_bytes} bytes"),
    };
    TransportError::new(TransportErrorKind::BodyTooLarge, message)
}
