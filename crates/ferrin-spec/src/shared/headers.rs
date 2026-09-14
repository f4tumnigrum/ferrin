//! Header map wrapper with merge semantics.

use std::fmt;

use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http::header::USER_AGENT;
use serde::Deserialize;
use serde::Serialize;

/// Header names whose values are masked in `Debug` and serialized output.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
    "x-goog-api-key",
    "cookie",
    "set-cookie",
];

/// Placeholder used in place of a sensitive header value.
const MASKED: &str = "***";

/// Request or response headers.
///
/// A thin wrapper around [`http::HeaderMap`] that adds provider-oriented
/// helpers: overriding merge, user-agent suffixing and masking of sensitive
/// values in diagnostic output. `Debug` and `Serialize` never reveal
/// authorization, API-key or cookie values.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Headers(HeaderMap);

impl Headers {
    /// Creates an empty header map.
    #[must_use]
    pub fn new() -> Self {
        Self(HeaderMap::new())
    }

    /// Wraps an existing [`HeaderMap`].
    #[must_use]
    pub fn from_map(map: HeaderMap) -> Self {
        Self(map)
    }

    /// Returns the wrapped [`HeaderMap`].
    #[must_use]
    pub fn as_map(&self) -> &HeaderMap {
        &self.0
    }

    /// Returns the wrapped [`HeaderMap`] mutably.
    pub fn as_map_mut(&mut self) -> &mut HeaderMap {
        &mut self.0
    }

    /// Consumes the wrapper and returns the [`HeaderMap`].
    #[must_use]
    pub fn into_map(self) -> HeaderMap {
        self.0
    }

    /// Returns `true` when no header is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of header entries (repeated names count once each).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the first value of `name` as a string, if present and ASCII.
    #[must_use]
    pub fn get_str(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(|value| value.to_str().ok())
    }

    /// Returns `true` when `name` is present.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Inserts a header, replacing existing values with the same name.
    ///
    /// Returns an error when the name or value is not a valid header.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHeader`] when `name` is not a valid header name or
    /// `value` contains characters not allowed in header values.
    pub fn insert(&mut self, name: &str, value: &str) -> Result<(), InvalidHeader> {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| InvalidHeader {
            name: name.to_owned(),
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| InvalidHeader {
            name: name.to_string(),
        })?;
        self.0.insert(name, value);
        Ok(())
    }

    /// Builder-style [`Headers::insert`]; invalid pairs are skipped silently.
    ///
    /// Intended for static configuration where the caller controls the values;
    /// use [`Headers::insert`] for untrusted input.
    #[must_use]
    pub fn with(mut self, name: &str, value: &str) -> Self {
        let _ = self.insert(name, value);
        self
    }

    /// Removes all values for `name`.
    pub fn remove(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// Merges `other` into `self`; values in `other` override existing ones.
    ///
    /// Repeated header names in `other` replace all existing values of that
    /// name, mirroring "later headers win" semantics for configuration
    /// layering (provider defaults, per-model settings, per-call headers).
    pub fn merge(&mut self, other: &Headers) {
        let mut current: Option<HeaderName> = None;
        for (name, value) in &other.0 {
            if current.as_ref() != Some(name) {
                self.0.remove(name);
                current = Some(name.clone());
            }
            self.0.append(name.clone(), value.clone());
        }
    }

    /// Returns a copy with `other` merged on top; see [`Headers::merge`].
    #[must_use]
    pub fn merged(mut self, other: &Headers) -> Self {
        self.merge(other);
        self
    }

    /// Merges optional string pairs; pairs whose value is `None` are skipped.
    ///
    /// Invalid names or values are skipped as well; this mirrors the lenient
    /// behaviour needed when layering user-supplied header maps.
    pub fn merge_pairs<'a, I>(&mut self, pairs: I)
    where
        I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
    {
        for (name, value) in pairs {
            if let Some(value) = value {
                let _ = self.insert(name, value);
            }
        }
    }

    /// Appends `suffixes` to the `user-agent` header, creating it if absent.
    ///
    /// Existing user-agent text is kept and each suffix is separated by a
    /// single space. Empty suffixes are ignored.
    pub fn with_user_agent_suffix<'a, I>(mut self, suffixes: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut parts: Vec<String> = Vec::new();
        if let Some(existing) = self.get_str(USER_AGENT.as_str()) {
            let existing = existing.trim();
            if !existing.is_empty() {
                parts.push(existing.to_owned());
            }
        }
        parts.extend(
            suffixes
                .into_iter()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
        let joined = parts.join(" ");
        if let Ok(value) = HeaderValue::from_str(&joined) {
            self.0.insert(USER_AGENT, value);
        }
        self
    }

    /// Iterates over `(name, value)` pairs with values rendered as strings.
    ///
    /// Non-ASCII values are rendered lossily; sensitive values are not masked.
    pub fn iter_str(&self) -> impl Iterator<Item = (&str, String)> + '_ {
        self.0.iter().map(|(name, value)| {
            (
                name.as_str(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
    }

    /// Returns a copy with sensitive values replaced by `***`.
    #[must_use]
    pub fn masked(&self) -> Headers {
        let mut masked = HeaderMap::with_capacity(self.0.len());
        for (name, value) in &self.0 {
            if is_sensitive(name) {
                masked.append(name.clone(), HeaderValue::from_static(MASKED));
            } else {
                masked.append(name.clone(), value.clone());
            }
        }
        Headers(masked)
    }
}

fn is_sensitive(name: &HeaderName) -> bool {
    SENSITIVE_HEADERS.contains(&name.as_str())
}

impl fmt::Debug for Headers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (name, value) in &self.0 {
            if is_sensitive(name) {
                map.entry(&name.as_str(), &MASKED);
            } else {
                map.entry(&name.as_str(), &String::from_utf8_lossy(value.as_bytes()));
            }
        }
        map.finish()
    }
}

impl From<HeaderMap> for Headers {
    fn from(map: HeaderMap) -> Self {
        Self(map)
    }
}

impl From<Headers> for HeaderMap {
    fn from(headers: Headers) -> Self {
        headers.0
    }
}

impl<'a> IntoIterator for &'a Headers {
    type Item = (&'a HeaderName, &'a HeaderValue);
    type IntoIter = http::header::Iter<'a, HeaderValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Serializes as an object of `name -> value` pairs with sensitive values
/// masked. Repeated header names are joined with `", "`.
impl Serialize for Headers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut entries: Vec<(&str, String)> = Vec::with_capacity(self.0.len());
        for name in self.0.keys() {
            let rendered = if is_sensitive(name) {
                MASKED.to_owned()
            } else {
                self.0
                    .get_all(name)
                    .iter()
                    .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            entries.push((name.as_str(), rendered));
        }
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (name, value) in entries {
            map.serialize_entry(name, &value)?;
        }
        map.end()
    }
}

/// Deserializes from an object of `name -> value` pairs; invalid entries are
/// rejected with an error.
impl<'de> Deserialize<'de> for Headers {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HeadersVisitor;

        impl<'de> serde::de::Visitor<'de> for HeadersVisitor {
            type Value = Headers;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of header names to string values")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Headers, A::Error> {
                let mut headers = Headers::new();
                while let Some((name, value)) = map.next_entry::<String, String>()? {
                    headers
                        .insert(&name, &value)
                        .map_err(serde::de::Error::custom)?;
                }
                Ok(headers)
            }
        }

        deserializer.deserialize_map(HeadersVisitor)
    }
}

/// Error returned when a header name or value is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid header `{name}`")]
pub struct InvalidHeader {
    /// The offending header name.
    pub name: String,
}
