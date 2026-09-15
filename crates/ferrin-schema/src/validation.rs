//! Dynamic JSON Schema validation.
//!
//! [`ValidationIssue`] / [`ValidationIssues`] are always available so custom
//! validators can report structured problems. The [`Validator`] that checks a
//! value against a raw JSON Schema requires the `json-schema-validation`
//! feature.

use std::fmt;

#[cfg(feature = "json-schema-validation")]
use serde_json::Value;

/// One validation problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// JSON pointer to the offending value (empty for the root).
    pub path: String,
    /// Human-readable message.
    pub message: String,
}

/// A set of validation problems, usable as an error cause.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationIssues {
    /// The problems, in schema evaluation order.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationIssues {
    /// Creates a set from issues.
    #[must_use]
    pub fn new(issues: Vec<ValidationIssue>) -> Self {
        Self { issues }
    }

    /// Creates a set with a single root-level message.
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            issues: vec![ValidationIssue {
                path: String::new(),
                message: message.into(),
            }],
        }
    }

    /// Returns `true` when there are no issues.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }
}

impl fmt::Display for ValidationIssues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.issues.as_slice() {
            [] => f.write_str("validation failed"),
            [single] if single.path.is_empty() => f.write_str(&single.message),
            [single] => write!(f, "{}: {}", single.path, single.message),
            many => {
                write!(f, "{} validation issues:", many.len())?;
                for issue in many {
                    if issue.path.is_empty() {
                        write!(f, " [{}]", issue.message)?;
                    } else {
                        write!(f, " [{}: {}]", issue.path, issue.message)?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ValidationIssues {}

/// A compiled JSON Schema validator using the declared dialect.
#[cfg(feature = "json-schema-validation")]
#[derive(Debug)]
pub struct Validator {
    inner: jsonschema::Validator,
}

#[cfg(feature = "json-schema-validation")]
impl Validator {
    /// Compiles `schema` (draft-07 unless the schema declares another draft).
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::InvalidSchema`](crate::SchemaError::InvalidSchema)
    /// when the schema is not a valid JSON Schema.
    pub fn compile(schema: &Value) -> Result<Self, crate::SchemaError> {
        let options = if schema.get("$schema").is_some() {
            jsonschema::options()
        } else {
            jsonschema::draft7::options()
        };
        let inner = options
            .build(schema)
            .map_err(|error| crate::SchemaError::InvalidSchema {
                message: error.to_string(),
            })?;
        Ok(Self { inner })
    }

    /// Returns `true` when `value` satisfies the schema.
    #[must_use]
    pub fn is_valid(&self, value: &Value) -> bool {
        self.inner.is_valid(value)
    }

    /// Collects every violation of `value` against the schema.
    #[must_use]
    pub fn issues(&self, value: &Value) -> ValidationIssues {
        let issues = self
            .inner
            .iter_errors(value)
            .map(|error| ValidationIssue {
                path: error.instance_path().to_string(),
                message: error.to_string(),
            })
            .collect();
        ValidationIssues::new(issues)
    }

    /// Validates `value`, returning all issues on failure.
    ///
    /// # Errors
    ///
    /// Returns the collected [`ValidationIssues`] when `value` violates the
    /// schema.
    pub fn validate(&self, value: &Value) -> Result<(), ValidationIssues> {
        let issues = self.issues(value);
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}
