//! Media type detection from magic numbers.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ferrin_spec::MediaType;
use ferrin_spec::error::UnsupportedFunctionalityError;

struct Signature {
    media_type: &'static str,
    prefix: &'static [Option<u8>],
}

macro_rules! sig {
    ($media:literal, [$($byte:tt),* $(,)?]) => {
        Signature { media_type: $media, prefix: &[$(sig!(@b $byte)),*] }
    };
    (@b _) => { None };
    (@b $byte:literal) => { Some($byte) };
}

const IMAGE: &[Signature] = &[
    sig!("image/gif", [0x47, 0x49, 0x46, 0x38, 0x37, 0x61]),
    sig!("image/gif", [0x47, 0x49, 0x46, 0x38, 0x39, 0x61]),
    sig!("image/png", [0x89, 0x50, 0x4e, 0x47]),
    sig!("image/jpeg", [0xff, 0xd8]),
    sig!(
        "image/webp",
        [0x52, 0x49, 0x46, 0x46, _, _, _, _, 0x57, 0x45, 0x42, 0x50]
    ),
    sig!(
        "image/bmp",
        [0x42, 0x4d, _, _, _, _, 0x00, 0x00, 0x00, 0x00]
    ),
    sig!("image/tiff", [0x49, 0x49, 0x2a, 0x00]),
    sig!("image/tiff", [0x4d, 0x4d, 0x00, 0x2a]),
    sig!(
        "image/avif",
        [
            0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66
        ]
    ),
    sig!(
        "image/heic",
        [
            0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x68, 0x65, 0x69, 0x63
        ]
    ),
];

const DOCUMENT: &[Signature] = &[sig!("application/pdf", [0x25, 0x50, 0x44, 0x46])];

const AUDIO_WITHOUT_MP4: &[Signature] = &[
    sig!("audio/mpeg", [0xff, 0xfb]),
    sig!("audio/mpeg", [0xff, 0xfa]),
    sig!("audio/mpeg", [0xff, 0xf3]),
    sig!("audio/mpeg", [0xff, 0xf2]),
    sig!("audio/mpeg", [0xff, 0xe3]),
    sig!("audio/mpeg", [0xff, 0xe2]),
    sig!(
        "audio/wav",
        [0x52, 0x49, 0x46, 0x46, _, _, _, _, 0x57, 0x41, 0x56, 0x45]
    ),
    sig!("audio/ogg", [0x4f, 0x67, 0x67, 0x53]),
    sig!("audio/flac", [0x66, 0x4c, 0x61, 0x43]),
    sig!("audio/aac", [0x40, 0x15, 0x00, 0x00]),
    sig!("audio/webm", [0x1a, 0x45, 0xdf, 0xa3]),
];

const AUDIO_MP4: &[Signature] = &[sig!(
    "audio/mp4",
    [0x00, 0x00, 0x00, _, 0x66, 0x74, 0x79, 0x70]
)];

const VIDEO: &[Signature] = &[
    sig!("video/mp4", [0x00, 0x00, 0x00, _, 0x66, 0x74, 0x79, 0x70]),
    sig!("video/webm", [0x1a, 0x45, 0xdf, 0xa3]),
    sig!(
        "video/quicktime",
        [0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70, 0x71, 0x74]
    ),
    sig!("video/x-msvideo", [0x52, 0x49, 0x46, 0x46]),
];

const DEFAULT_SNIFF_BYTES: usize = 18;
const MAX_SIGNATURE_BYTES: usize = 12;
/// Largest ID3 tag that is skipped before sniffing audio.
pub const MAX_ID3_TAG_BYTES: usize = 128 * 1024;
const ID3_SCAN_BYTES: usize = MAX_ID3_TAG_BYTES + MAX_SIGNATURE_BYTES;

fn strip_id3(bytes: &[u8]) -> &[u8] {
    if bytes.len() > 10 && bytes[0] == b'I' && bytes[1] == b'D' && bytes[2] == b'3' {
        let size = (usize::from(bytes[6] & 0x7f) << 21)
            | (usize::from(bytes[7] & 0x7f) << 14)
            | (usize::from(bytes[8] & 0x7f) << 7)
            | usize::from(bytes[9] & 0x7f);
        bytes.get(size + 10..).unwrap_or(&[])
    } else {
        bytes
    }
}

fn detect_by(bytes: &[u8], tables: &[&[Signature]]) -> Option<MediaType> {
    let head = if bytes.starts_with(b"ID3") {
        strip_id3(&bytes[..bytes.len().min(ID3_SCAN_BYTES)])
    } else {
        &bytes[..bytes.len().min(DEFAULT_SNIFF_BYTES)]
    };
    for table in tables {
        for signature in *table {
            if head.len() >= signature.prefix.len()
                && signature
                    .prefix
                    .iter()
                    .zip(head)
                    .all(|(expected, actual)| expected.is_none_or(|byte| byte == *actual))
            {
                return Some(MediaType::new(signature.media_type));
            }
        }
    }
    None
}

/// Detects the media type of `bytes` across images, PDF, audio (except
/// `audio/mp4`, which is ambiguous with `video/mp4`) and video.
#[must_use]
pub fn detect_media_type(bytes: &[u8]) -> Option<MediaType> {
    detect_by(bytes, &[IMAGE, DOCUMENT, AUDIO_WITHOUT_MP4, VIDEO])
}

/// Detects the media type of `bytes` within one top-level type (`image`,
/// `audio`, `video` or `application`).
#[must_use]
pub fn detect_media_type_for(bytes: &[u8], top_level: &str) -> Option<MediaType> {
    match top_level {
        "image" => detect_by(bytes, &[IMAGE]),
        "audio" => detect_by(bytes, &[AUDIO_WITHOUT_MP4, AUDIO_MP4]),
        "video" => detect_by(bytes, &[VIDEO]),
        "application" => detect_by(bytes, &[DOCUMENT]),
        _ => None,
    }
}

/// Like [`detect_media_type_for`] for base64 text; only the prefix needed for
/// sniffing is decoded. `top_level` of `None` searches every table.
#[must_use]
pub fn detect_media_type_base64(text: &str, top_level: Option<&str>) -> Option<MediaType> {
    let max_bytes = if text.starts_with("SUQz") {
        ID3_SCAN_BYTES
    } else {
        DEFAULT_SNIFF_BYTES
    };
    let max_chars = max_bytes.div_ceil(3) * 4;
    let mut end = text.len().min(max_chars);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &text[..end];
    let bytes = BASE64_STANDARD
        .decode(prefix)
        .or_else(|_| base64::prelude::BASE64_STANDARD_NO_PAD.decode(prefix.trim_end_matches('=')))
        .ok()?;
    match top_level {
        Some(top_level) => detect_media_type_for(&bytes, top_level),
        None => detect_media_type(&bytes),
    }
}

/// Maps a media type to a file extension (`audio/mpeg` → `mp3`).
#[must_use]
pub fn media_type_to_extension(media_type: &MediaType) -> String {
    let subtype = media_type
        .as_str()
        .to_ascii_lowercase()
        .split_once('/')
        .map(|(_, subtype)| subtype.to_owned())
        .unwrap_or_default();
    match subtype.as_str() {
        "mpeg" => "mp3".to_owned(),
        "x-wav" => "wav".to_owned(),
        "opus" => "ogg".to_owned(),
        "mp4" | "x-m4a" => "m4a".to_owned(),
        _ => subtype,
    }
}

/// Resolves a possibly partial media type (`image`, `image/*`) to a full one
/// by sniffing inline bytes.
///
/// # Errors
///
/// Returns [`UnsupportedFunctionalityError`] when the subtype cannot be
/// determined (no inline bytes, or unknown signature).
pub fn resolve_full_media_type(
    media_type: &MediaType,
    inline_bytes: Option<&[u8]>,
) -> Result<MediaType, UnsupportedFunctionalityError> {
    if media_type.is_full() {
        return Ok(media_type.clone());
    }
    match inline_bytes {
        Some(bytes) => detect_media_type_for(bytes, &media_type.top_level()).ok_or_else(|| {
            UnsupportedFunctionalityError::new(format!(
                "file of media type \"{media_type}\" must specify subtype since it could not be auto-detected"
            ))
        }),
        None => Err(UnsupportedFunctionalityError::new(format!(
            "file of media type \"{media_type}\" must specify subtype since it is not passed as inline bytes"
        ))),
    }
}
