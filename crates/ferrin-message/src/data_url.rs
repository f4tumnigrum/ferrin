//! RFC 2397 `data:` URL parsing.
//!
//! Rules: the scheme is matched case-insensitively; the first comma separates
//! the header from the payload; a `base64` parameter (case-insensitive) marks
//! a base64 payload, which is percent-decoded, stripped of ASCII whitespace
//! and decoded with padding optional; other payloads are percent-decoded; an
//! empty media type defaults to `text/plain;charset=US-ASCII`; media type
//! parameters other than `base64` are kept.

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::DecodePaddingMode;
use base64::engine::GeneralPurpose;
use base64::engine::GeneralPurposeConfig;
use bytes::Bytes;
use ferrin_spec::MediaType;
use percent_encoding::percent_decode_str;

use crate::error::InvalidDataContentError;

/// Media type used when a data URL omits it.
pub const DEFAULT_MEDIA_TYPE: &str = "text/plain;charset=US-ASCII";

const LENIENT_BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A parsed `data:` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataUrl {
    /// Media type including any parameters other than `base64`.
    pub media_type: MediaType,
    /// Decoded payload.
    pub data: Bytes,
    /// Whether the payload was base64-encoded.
    pub is_base64: bool,
}

/// Returns `true` when `input` starts with the `data:` scheme.
#[must_use]
pub fn is_data_url(input: &str) -> bool {
    input.len() >= 5 && input.as_bytes()[..5].eq_ignore_ascii_case(b"data:")
}

/// Parses a `data:` URL.
///
/// # Errors
///
/// Returns [`InvalidDataContentError`] when the scheme is missing, the URL
/// has no comma, or a base64 payload does not decode.
pub fn parse(input: &str) -> Result<DataUrl, InvalidDataContentError> {
    if !is_data_url(input) {
        return Err(InvalidDataContentError::new(
            "data url must start with `data:`",
        ));
    }
    let rest = &input[5..];
    let Some((header, payload)) = rest.split_once(',') else {
        return Err(InvalidDataContentError::new(
            "data url has no `,` separator",
        ));
    };

    let mut is_base64 = false;
    let mut media_type = String::new();
    for (index, segment) in header.split(';').enumerate() {
        let segment = segment.trim();
        if index == 0 {
            media_type.push_str(segment);
        } else if segment.eq_ignore_ascii_case("base64") {
            is_base64 = true;
        } else if !segment.is_empty() {
            if media_type.is_empty() {
                media_type.push_str("text/plain");
            }
            media_type.push(';');
            media_type.push_str(segment);
        }
    }
    if media_type.is_empty() {
        media_type.push_str(DEFAULT_MEDIA_TYPE);
    }

    let decoded = percent_decode_str(payload).collect::<Vec<u8>>();
    let data = if is_base64 {
        let compact: Vec<u8> = decoded
            .into_iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        LENIENT_BASE64
            .decode(compact)
            .map(Bytes::from)
            .map_err(|error| {
                InvalidDataContentError::new("data url payload is not valid base64")
                    .with_cause(error)
            })?
    } else {
        Bytes::from(decoded)
    };

    Ok(DataUrl {
        media_type: MediaType::new(media_type),
        data,
        is_base64,
    })
}
