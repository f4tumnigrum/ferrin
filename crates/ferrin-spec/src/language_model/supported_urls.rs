//! URL patterns a provider can fetch itself.

use regex::Regex;
use url::Url;

use crate::shared::MediaType;

/// URL patterns, grouped by media type pattern, that the provider fetches
/// directly instead of receiving inline bytes.
///
/// Keys are media type patterns: a full type (`image/png`), a wildcard
/// (`image/*`) or `*` / `*/*` for any type. Values are regular expressions
/// matched against the lowercased URL.
#[derive(Debug, Clone, Default)]
pub struct SupportedUrls {
    entries: Vec<(String, Vec<Regex>)>,
}

impl SupportedUrls {
    /// Creates an empty set: every file is downloaded and inlined by the core.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Creates a set that accepts every URL for every media type.
    ///
    /// # Panics
    ///
    /// Does not panic: the pattern is a constant that always compiles.
    #[must_use]
    pub fn all() -> Self {
        Self::default().with("*", [any_url()])
    }

    /// Adds `patterns` for `media_type_pattern`.
    #[must_use]
    pub fn with(
        mut self,
        media_type_pattern: &str,
        patterns: impl IntoIterator<Item = Regex>,
    ) -> Self {
        self.insert(media_type_pattern, patterns);
        self
    }

    /// Adds `patterns` for `media_type_pattern`, extending an existing entry.
    pub fn insert(&mut self, media_type_pattern: &str, patterns: impl IntoIterator<Item = Regex>) {
        let key = media_type_pattern.trim().to_ascii_lowercase();
        if let Some((_, existing)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            existing.extend(patterns);
        } else {
            self.entries.push((key, patterns.into_iter().collect()));
        }
    }

    /// Returns `true` when no pattern is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(|(_, patterns)| patterns.is_empty())
    }

    /// Iterates over `(media type pattern, url patterns)` entries.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Regex])> + '_ {
        self.entries
            .iter()
            .map(|(key, patterns)| (key.as_str(), patterns.as_slice()))
    }

    /// Returns `true` when the provider can fetch `url` for `media_type`.
    ///
    /// A top-level media type (`image`) matches wildcard entries of the same
    /// top-level type (`image/*`) but not full entries (`image/png`).
    #[must_use]
    pub fn supports(&self, media_type: &MediaType, url: &Url) -> bool {
        let url = url.as_str().to_ascii_lowercase();
        let media_type = media_type.as_str().trim().to_ascii_lowercase();
        let is_top_level_only = !media_type.contains('/');

        self.entries
            .iter()
            .filter(|(key, _)| {
                let prefix = if key == "*" || key == "*/*" {
                    String::new()
                } else {
                    key.replacen('*', "", 1)
                };
                if prefix.is_empty() {
                    return true;
                }
                if is_top_level_only {
                    return format!("{media_type}/") == prefix;
                }
                media_type.starts_with(&prefix)
            })
            .flat_map(|(_, patterns)| patterns.iter())
            .any(|pattern| pattern.is_match(&url))
    }
}

/// A pattern matching any URL.
#[must_use]
pub fn any_url() -> Regex {
    #[allow(clippy::expect_used, reason = "constant pattern always compiles")]
    Regex::new(".*").expect("constant regex")
}
