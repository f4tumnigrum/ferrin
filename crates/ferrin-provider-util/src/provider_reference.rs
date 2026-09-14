//! Provider reference resolution.

use ferrin_spec::ProviderReference;
use ferrin_spec::error::NoSuchProviderReferenceError;

/// Returns the id stored for `provider` in `reference`.
///
/// # Errors
///
/// Returns [`NoSuchProviderReferenceError`] when the reference has no entry
/// for the provider.
pub fn resolve_provider_reference<'a>(
    reference: &'a ProviderReference,
    provider: &str,
) -> Result<&'a str, NoSuchProviderReferenceError> {
    reference
        .get(provider)
        .map(String::as_str)
        .ok_or_else(|| NoSuchProviderReferenceError::new(provider, reference.clone()))
}
