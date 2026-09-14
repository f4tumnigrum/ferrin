//! Identifier generation.

use rand::RngExt;

/// Alphabet of generated ids (digits, upper- and lower-case ASCII letters).
pub const DEFAULT_ID_ALPHABET: &str =
    "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Default length of the random part.
pub const DEFAULT_ID_SIZE: usize = 16;

/// Produces identifiers.
///
/// Implement this to make ids deterministic in tests.
pub trait IdGenerator: Send + Sync {
    /// Returns a new identifier.
    fn generate(&self) -> String;
}

impl<F: Fn() -> String + Send + Sync> IdGenerator for F {
    fn generate(&self) -> String {
        self()
    }
}

/// Generates `prefix<separator><random>` ids from a fixed alphabet.
#[derive(Debug, Clone)]
pub struct PrefixedIdGenerator {
    prefix: Option<String>,
    separator: char,
    size: usize,
    alphabet: &'static str,
}

impl PrefixedIdGenerator {
    /// Ids of the form `prefix-<size random characters>`.
    #[must_use]
    pub fn new(prefix: impl Into<String>, size: usize) -> Self {
        Self {
            prefix: Some(prefix.into()),
            separator: '-',
            size,
            alphabet: DEFAULT_ID_ALPHABET,
        }
    }

    /// Ids without a prefix.
    #[must_use]
    pub fn unprefixed(size: usize) -> Self {
        Self {
            prefix: None,
            separator: '-',
            size,
            alphabet: DEFAULT_ID_ALPHABET,
        }
    }

    /// Changes the separator between prefix and random part.
    #[must_use]
    pub fn with_separator(mut self, separator: char) -> Self {
        self.separator = separator;
        self
    }

    /// The prefix, if any.
    #[must_use]
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }
}

impl Default for PrefixedIdGenerator {
    fn default() -> Self {
        Self::unprefixed(DEFAULT_ID_SIZE)
    }
}

impl IdGenerator for PrefixedIdGenerator {
    fn generate(&self) -> String {
        let random = random_string(self.size, self.alphabet);
        match &self.prefix {
            Some(prefix) => format!("{prefix}{}{random}", self.separator),
            None => random,
        }
    }
}

/// Generates a random 16-character id.
#[must_use]
pub fn generate_id() -> String {
    random_string(DEFAULT_ID_SIZE, DEFAULT_ID_ALPHABET)
}

fn random_string(size: usize, alphabet: &str) -> String {
    let chars: Vec<char> = alphabet.chars().collect();
    let mut rng = rand::rng();
    (0..size)
        .map(|_| chars[rng.random_range(0..chars.len())])
        .collect()
}
