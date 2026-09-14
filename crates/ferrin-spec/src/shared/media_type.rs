//! Media type wrapper.

use serde::Deserialize;
use serde::Serialize;

/// An IANA media type such as `image/png`, or a top-level type such as `image`.
///
/// The wrapper does not validate syntax; it provides normalization helpers
/// used when matching provider capabilities. Comparison is case-sensitive on
/// the stored string; callers should normalize before comparing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MediaType(String);

impl MediaType {
    /// Creates a media type from any string-like value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the media type as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the media type and returns the inner `String`.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// Returns `true` when the value is a full `type/subtype` media type.
    ///
    /// A wildcard subtype (`image/*`) is not considered full.
    #[must_use]
    pub fn is_full(&self) -> bool {
        match self.0.split_once('/') {
            Some((top, sub)) => !top.is_empty() && !sub.is_empty() && sub != "*",
            None => false,
        }
    }

    /// Returns the top-level type (`image` for `image/png`), lowercased.
    #[must_use]
    pub fn top_level(&self) -> String {
        let top = self
            .0
            .split_once('/')
            .map_or(self.0.as_str(), |(top, _)| top);
        top.trim().to_ascii_lowercase()
    }

    /// Returns the subtype (`png` for `image/png`) without parameters, if any.
    #[must_use]
    pub fn subtype(&self) -> Option<&str> {
        let (_, sub) = self.0.split_once('/')?;
        let sub = sub.split(';').next().unwrap_or(sub).trim();
        (!sub.is_empty()).then_some(sub)
    }

    /// Normalizes the media type for capability matching.
    ///
    /// Lowercases the value, drops parameters (`; charset=utf-8`) and turns a
    /// wildcard subtype (`image/*`) into its top-level type (`image`).
    #[must_use]
    pub fn normalize(&self) -> MediaType {
        let without_params = self.0.split(';').next().unwrap_or(&self.0).trim();
        let lower = without_params.to_ascii_lowercase();
        match lower.split_once('/') {
            Some((top, "*")) => MediaType(top.to_owned()),
            Some((top, "")) => MediaType(top.to_owned()),
            _ => MediaType(lower),
        }
    }

    /// Returns `true` when this media type matches `pattern`.
    ///
    /// `pattern` may be a full type, a wildcard (`image/*`) or a top-level
    /// type (`image`). Matching is case-insensitive and ignores parameters.
    #[must_use]
    pub fn matches(&self, pattern: &MediaType) -> bool {
        let this = self.normalize();
        let pattern = pattern.normalize();
        if this == pattern {
            return true;
        }
        if pattern.0.contains('/') {
            return false;
        }
        this.top_level() == pattern.0
    }
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MediaType {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MediaType {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<MediaType> for String {
    fn from(value: MediaType) -> Self {
        value.0
    }
}

impl AsRef<str> for MediaType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for MediaType {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for MediaType {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
