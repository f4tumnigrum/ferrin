//! Schema-layer error type.

use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::JsonParseError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TypeValidationError;

/// Errors produced while parsing or validating JSON against a schema.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// The text is not valid JSON.
    #[error(transparent)]
    JsonParse(#[from] JsonParseError),
    /// The value does not match the schema or type.
    #[error(transparent)]
    TypeValidation(#[from] TypeValidationError),
    /// A valid schema shape cannot be preserved by the requested transform.
    #[error("unsupported json schema keyword {keyword} for {transform}")]
    UnsupportedTransform {
        /// Transform that cannot represent this schema.
        transform: &'static str,
        /// Unsupported schema keyword.
        keyword: &'static str,
    },
    /// The JSON Schema itself is invalid.
    #[error("invalid json schema: {message}")]
    InvalidSchema {
        /// Explanation.
        message: String,
    },
}

impl From<SchemaError> for ProviderError {
    fn from(error: SchemaError) -> Self {
        match error {
            SchemaError::JsonParse(error) => Self::from(error),
            SchemaError::TypeValidation(error) => Self::from(error),
            error @ SchemaError::UnsupportedTransform { .. } => {
                Self::from(InvalidArgumentError::new("schema", error.to_string()))
            }
            SchemaError::InvalidSchema { message } => {
                Self::from(InvalidArgumentError::new("schema", message))
            }
        }
    }
}
