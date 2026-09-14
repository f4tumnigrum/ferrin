//! File payloads shared by prompts, results and service interfaces.

use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use super::ProviderReference;
use super::base64_bytes;

/// The payload of a file: inline bytes, a URL, a provider reference or an
/// inline text document.
///
/// Wire format is tagged by `type`: `{"type": "data", "data": "<base64>"}`,
/// `{"type": "url", "url": "..."}`, `{"type": "reference", "reference": {..}}`
/// or `{"type": "text", "text": "..."}`. Generated files (results, stream
/// parts) only use the `data` and `url` forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FileData {
    /// Inline bytes, serialized as standard base64 with padding.
    #[serde(rename = "data")]
    Bytes {
        /// The bytes.
        #[serde(with = "base64_bytes")]
        data: Bytes,
    },
    /// A URL the provider fetches itself.
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
}

impl FileData {
    /// Inline bytes.
    #[must_use]
    pub fn bytes(data: impl Into<Bytes>) -> Self {
        Self::Bytes { data: data.into() }
    }

    /// Inline bytes from a base64 string (standard alphabet, padding optional).
    ///
    /// # Errors
    ///
    /// Returns the decoding error when `text` is not valid base64.
    pub fn from_base64(text: &str) -> Result<Self, base64::DecodeError> {
        base64_bytes::decode(text).map(|data| Self::Bytes { data })
    }

    /// A URL.
    #[must_use]
    pub fn url(url: Url) -> Self {
        Self::Url { url }
    }

    /// A reference to a file uploaded to a single provider.
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

    /// Returns the bytes when inline.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            Self::Bytes { data } => Some(data),
            Self::Url { .. } | Self::Reference { .. } | Self::Text { .. } => None,
        }
    }

    /// Returns the URL when this is a URL payload.
    #[must_use]
    pub fn as_url(&self) -> Option<&Url> {
        match self {
            Self::Url { url } => Some(url),
            Self::Bytes { .. } | Self::Reference { .. } | Self::Text { .. } => None,
        }
    }

    /// Returns the provider reference when this is a reference payload.
    #[must_use]
    pub fn as_reference(&self) -> Option<&ProviderReference> {
        match self {
            Self::Reference { reference } => Some(reference),
            Self::Bytes { .. } | Self::Url { .. } | Self::Text { .. } => None,
        }
    }

    /// Returns the text when this is an inline text document.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::Bytes { .. } | Self::Url { .. } | Self::Reference { .. } => None,
        }
    }

    /// Returns the inline bytes encoded as standard base64.
    #[must_use]
    pub fn to_base64(&self) -> Option<String> {
        self.as_bytes().map(|bytes| base64_bytes::encode(bytes))
    }
}

impl From<Bytes> for FileData {
    fn from(data: Bytes) -> Self {
        Self::Bytes { data }
    }
}

impl From<Vec<u8>> for FileData {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes {
            data: Bytes::from(bytes),
        }
    }
}

impl From<Url> for FileData {
    fn from(url: Url) -> Self {
        Self::Url { url }
    }
}

impl From<ProviderReference> for FileData {
    fn from(reference: ProviderReference) -> Self {
        Self::Reference { reference }
    }
}
