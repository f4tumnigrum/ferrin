//! Policy path normalization shared by the clients.

use crate::error::PolicyError;

/// A policy path split into segments.
///
/// Accepts the OPA REST form `ferrin/tools/decision` and the Rego form
/// `ferrin.tools.decision`, with or without a leading `/` or `data`
/// segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolicyPath {
    segments: Vec<String>,
}

impl PolicyPath {
    /// Parses `path`.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidPath`] when no segment remains or a
    /// segment is empty or contains whitespace.
    pub(crate) fn parse(path: &str) -> Result<Self, PolicyError> {
        let invalid = |message: &str| PolicyError::InvalidPath {
            path: path.to_owned(),
            message: message.to_owned(),
        };
        let trimmed = path.trim().trim_start_matches('/');
        let mut segments: Vec<String> = trimmed.split(['/', '.']).map(str::to_owned).collect();
        if segments.first().is_some_and(|first| first == "data") {
            segments.remove(0);
        }
        if segments.iter().any(String::is_empty) {
            return Err(invalid("empty segment"));
        }
        if segments
            .iter()
            .any(|segment| segment.chars().any(char::is_whitespace))
        {
            return Err(invalid("segment contains whitespace"));
        }
        if segments.is_empty() {
            return Err(invalid("no rule segment"));
        }
        Ok(Self { segments })
    }

    /// The segments without the `data` root.
    pub(crate) fn segments(&self) -> &[String] {
        &self.segments
    }

    /// The Rego rule reference, for example `data.ferrin.tools.decision`.
    #[cfg(feature = "rego")]
    pub(crate) fn rego_rule(&self) -> String {
        format!("data.{}", self.segments.join("."))
    }
}
