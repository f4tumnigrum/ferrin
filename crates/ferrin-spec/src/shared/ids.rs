//! Newtype identifiers.
//!
//! Each identifier wraps a `String` and serializes transparently as a JSON
//! string. Distinct types prevent, for example, passing a tool name where a
//! tool call id is expected.

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new identifier from any string-like value.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the identifier as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identifier and returns the inner `String`.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

string_id! {
    /// Provider identifier, for example `openai.responses` or `anthropic.messages`.
    ProviderId
}

string_id! {
    /// Model identifier as understood by the provider, for example `gpt-5`.
    ModelId
}

string_id! {
    /// Name of a tool as exposed to the model.
    ToolName
}

string_id! {
    /// Identifier of a tool call, assigned by the provider.
    ToolCallId
}

string_id! {
    /// Identifier of a tool approval request.
    ApprovalId
}

string_id! {
    /// Identifier of a provider batch job.
    BatchId
}

string_id! {
    /// Identifier of a text, reasoning or tool-input part within a stream.
    ///
    /// Provider-assigned part ids are unique within a single call only; the
    /// core remaps colliding ids across steps.
    PartId
}
