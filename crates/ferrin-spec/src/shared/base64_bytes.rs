//! Serde adapter that encodes [`bytes::Bytes`] as a standard base64 string.
//!
//! Used with `#[serde(with = "base64_bytes")]`. Binary data in the wire format
//! is always the standard alphabet with padding.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use bytes::Bytes;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serializer;

/// Serializes `bytes` as a base64 string.
pub(crate) fn serialize<S: Serializer>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&BASE64_STANDARD.encode(bytes))
}

/// Deserializes a base64 string into `Bytes`.
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Bytes, D::Error> {
    let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
    decode(&text).map_err(serde::de::Error::custom)
}

/// Decodes a base64 string (standard alphabet, padding optional) into `Bytes`.
pub(crate) fn decode(text: &str) -> Result<Bytes, base64::DecodeError> {
    match BASE64_STANDARD.decode(text) {
        Ok(bytes) => Ok(Bytes::from(bytes)),
        Err(base64::DecodeError::InvalidPadding) => {
            let trimmed = text.trim_end_matches('=');
            base64::prelude::BASE64_STANDARD_NO_PAD
                .decode(trimmed)
                .map(Bytes::from)
        }
        Err(err) => Err(err),
    }
}

/// Encodes bytes as a standard base64 string with padding.
pub(crate) fn encode(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}
