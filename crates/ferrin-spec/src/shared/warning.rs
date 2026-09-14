//! Non-fatal warnings emitted by adapters.

use serde::Deserialize;
use serde::Serialize;

/// A non-fatal warning produced while preparing or executing a call.
///
/// Adapters emit warnings instead of errors when a requested option is not
/// supported by the provider; the option is ignored and the call proceeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Warning {
    /// A feature or setting is not supported by the provider or model.
    Unsupported {
        /// Name of the unsupported feature or setting.
        feature: String,
        /// Additional details.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    /// A feature is supported with reduced fidelity or via a workaround.
    Compatibility {
        /// Name of the affected feature.
        feature: String,
        /// Additional details.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    /// A setting is deprecated and will be removed.
    Deprecated {
        /// Name of the deprecated setting.
        setting: String,
        /// Migration guidance.
        message: String,
    },
    /// Any other warning.
    Other {
        /// Human-readable message.
        message: String,
    },
}

impl Warning {
    /// Creates an [`Warning::Unsupported`] warning without details.
    #[must_use]
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            details: None,
        }
    }

    /// Creates an [`Warning::Unsupported`] warning with details.
    #[must_use]
    pub fn unsupported_with_details(
        feature: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            details: Some(details.into()),
        }
    }

    /// Creates a [`Warning::Compatibility`] warning.
    #[must_use]
    pub fn compatibility(feature: impl Into<String>, details: Option<String>) -> Self {
        Self::Compatibility {
            feature: feature.into(),
            details,
        }
    }

    /// Creates a [`Warning::Deprecated`] warning.
    #[must_use]
    pub fn deprecated(setting: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Deprecated {
            setting: setting.into(),
            message: message.into(),
        }
    }

    /// Creates a [`Warning::Other`] warning.
    #[must_use]
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { feature, details } => match details {
                Some(details) => write!(f, "unsupported {feature}: {details}"),
                None => write!(f, "unsupported {feature}"),
            },
            Self::Compatibility { feature, details } => match details {
                Some(details) => write!(f, "compatibility for {feature}: {details}"),
                None => write!(f, "compatibility for {feature}"),
            },
            Self::Deprecated { setting, message } => {
                write!(f, "deprecated setting {setting}: {message}")
            }
            Self::Other { message } => f.write_str(message),
        }
    }
}
