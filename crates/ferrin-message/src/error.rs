//! Message-layer errors.

use std::path::PathBuf;

/// Binary or data-URL content could not be decoded.
#[derive(Debug, thiserror::Error)]
#[error("invalid data content: {message}")]
pub struct InvalidDataContentError {
    /// Explanation.
    pub message: String,
    /// Underlying decoding error, if any.
    #[source]
    pub cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl InvalidDataContentError {
    /// Creates an error from a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cause: None,
        }
    }

    /// Attaches the underlying cause.
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

/// A [`FileSource`](crate::FileSource) could not be converted into provider
/// file data.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FileSourceError {
    /// The inline content is not valid base64 or a valid data URL.
    #[error(transparent)]
    InvalidDataContent(#[from] InvalidDataContentError),
    /// A local path must be read by the caller before conversion.
    #[error("file source path `{}` must be read before conversion", path.display())]
    UnreadPath {
        /// The path.
        path: PathBuf,
    },
}
