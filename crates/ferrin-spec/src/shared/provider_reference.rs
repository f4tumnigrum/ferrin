//! Provider-side resource references.

use std::collections::BTreeMap;

/// Provider-side resource identifiers, keyed by provider name.
///
/// Example: `{ "openai": "file-abc123" }`. A reference produced by one
/// provider's file upload can be attached to a message and resolved by the
/// same provider on a later call.
pub type ProviderReference = BTreeMap<String, String>;
