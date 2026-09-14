//! Application-side file payloads.

use std::path::PathBuf;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use bytes::Bytes;
use ferrin_spec::FileData;
use ferrin_spec::ProviderReference;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use crate::data_url;
use crate::data_url::DataUrl;
use crate::error::FileSourceError;
use crate::error::InvalidDataContentError;

/// Where the bytes of a file part come from.
///
/// Compared with the provider-level [`FileData`], applications may also pass
/// base64 text (decoded during conversion) and a local path (read during
/// conversion). Wire format is tagged by `type`: `data` (base64 bytes),
/// `base64`, `url`, `reference`, `text`, `path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FileSource {
    /// Inline bytes.
    #[serde(rename = "data")]
    Bytes {
        /// The bytes (serialized as standard base64).
        #[serde(with = "base64_codec")]
        data: Bytes,
    },
    /// Base64 text, decoded during conversion.
    Base64 {
        /// Standard-alphabet base64, padding optional, whitespace ignored.
        data: String,
    },
    /// A URL, including `data:` URLs.
    Url {
        /// The URL.
        url: Url,
    },
    /// A file previously uploaded to one or more providers.
    Reference {
        /// Provider key → provider-specific file id.
        reference: ProviderReference,
    },
    /// An inline text document.
    Text {
        /// The text.
        text: String,
    },
    /// A local file read during conversion.
    Path {
        /// The path.
        path: PathBuf,
    },
}

impl FileSource {
    /// Inline bytes.
    #[must_use]
    pub fn bytes(data: impl Into<Bytes>) -> Self {
        Self::Bytes { data: data.into() }
    }

    /// Base64 text.
    #[must_use]
    pub fn base64(data: impl Into<String>) -> Self {
        Self::Base64 { data: data.into() }
    }

    /// A URL.
    #[must_use]
    pub fn url(url: Url) -> Self {
        Self::Url { url }
    }

    /// A single-provider reference.
    #[must_use]
    pub fn reference(provider: impl Into<String>, id: impl Into<String>) -> Self {
        let mut map = ProviderReference::new();
        map.insert(provider.into(), id.into());
        Self::Reference { reference: map }
    }

    /// An inline text document.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// A local path.
    #[must_use]
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path { path: path.into() }
    }

    /// Parses a URL string into a URL source.
    ///
    /// # Errors
    ///
    /// Returns the parse error when `url` is not an absolute URL.
    pub fn parse_url(url: &str) -> Result<Self, url::ParseError> {
        Url::parse(url).map(|url| Self::Url { url })
    }

    /// Returns the inline bytes when this is a [`FileSource::Bytes`].
    #[must_use]
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            Self::Bytes { data } => Some(data),
            _ => None,
        }
    }

    /// Returns the URL when this is a [`FileSource::Url`].
    #[must_use]
    pub fn as_url(&self) -> Option<&Url> {
        match self {
            Self::Url { url } => Some(url),
            _ => None,
        }
    }

    /// Returns the provider reference when this is a [`FileSource::Reference`].
    #[must_use]
    pub fn as_reference(&self) -> Option<&ProviderReference> {
        match self {
            Self::Reference { reference } => Some(reference),
            _ => None,
        }
    }

    /// Returns `true` for a `data:` URL.
    #[must_use]
    pub fn is_data_url(&self) -> bool {
        matches!(self, Self::Url { url } if url.scheme().eq_ignore_ascii_case("data"))
    }

    /// Parses the `data:` URL when this is one.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidDataContentError`] when the data URL is malformed.
    pub fn data_url(&self) -> Option<Result<DataUrl, InvalidDataContentError>> {
        match self {
            Self::Url { url } if url.scheme().eq_ignore_ascii_case("data") => {
                Some(data_url::parse(url.as_str()))
            }
            _ => None,
        }
    }

    /// Decodes inline content (`Bytes` or `Base64`) into bytes.
    ///
    /// Returns `None` for sources that are not inline binary content.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidDataContentError`] when base64 text does not decode.
    pub fn decoded_bytes(&self) -> Result<Option<Bytes>, InvalidDataContentError> {
        match self {
            Self::Bytes { data } => Ok(Some(data.clone())),
            Self::Base64 { data } => decode_base64(data).map(Some),
            _ => Ok(None),
        }
    }
}

impl From<FileData> for FileSource {
    fn from(data: FileData) -> Self {
        match data {
            FileData::Bytes { data } => Self::Bytes { data },
            FileData::Url { url } => Self::Url { url },
            FileData::Reference { reference } => Self::Reference { reference },
            FileData::Text { text } => Self::Text { text },
            // `FileData` is `#[non_exhaustive]`; both crates ship from the same
            // workspace and a new variant is mirrored here in the same change.
            _ => unreachable!("every FileData variant has a FileSource counterpart"),
        }
    }
}

impl TryFrom<FileSource> for FileData {
    type Error = FileSourceError;

    /// Converts without I/O: base64 is decoded, URLs (including `data:`) are
    /// passed through, and paths are rejected with
    /// [`FileSourceError::UnreadPath`].
    fn try_from(source: FileSource) -> Result<Self, Self::Error> {
        match source {
            FileSource::Bytes { data } => Ok(Self::Bytes { data }),
            FileSource::Base64 { data } => Ok(Self::Bytes {
                data: decode_base64(&data)?,
            }),
            FileSource::Url { url } => Ok(Self::Url { url }),
            FileSource::Reference { reference } => Ok(Self::Reference { reference }),
            FileSource::Text { text } => Ok(Self::Text { text }),
            FileSource::Path { path } => Err(FileSourceError::UnreadPath { path }),
        }
    }
}

impl From<Bytes> for FileSource {
    fn from(data: Bytes) -> Self {
        Self::Bytes { data }
    }
}

impl From<Vec<u8>> for FileSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes {
            data: Bytes::from(bytes),
        }
    }
}

impl From<Url> for FileSource {
    fn from(url: Url) -> Self {
        Self::Url { url }
    }
}

impl From<ProviderReference> for FileSource {
    fn from(reference: ProviderReference) -> Self {
        Self::Reference { reference }
    }
}

impl From<PathBuf> for FileSource {
    fn from(path: PathBuf) -> Self {
        Self::Path { path }
    }
}

/// Decodes standard base64 with optional padding, ignoring ASCII whitespace.
fn decode_base64(text: &str) -> Result<Bytes, InvalidDataContentError> {
    use base64::alphabet::STANDARD;
    use base64::engine::DecodePaddingMode;
    use base64::engine::GeneralPurpose;
    use base64::engine::GeneralPurposeConfig;

    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let compact: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    LENIENT.decode(compact).map(Bytes::from).map_err(|error| {
        InvalidDataContentError::new("content is not valid base64").with_cause(error)
    })
}

mod base64_codec {
    use super::BASE64_STANDARD;
    use super::Engine;
    use bytes::Bytes;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub(super) fn serialize<S: Serializer>(
        bytes: &Bytes,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64_STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Bytes, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        super::decode_base64(&text).map_err(serde::de::Error::custom)
    }
}
