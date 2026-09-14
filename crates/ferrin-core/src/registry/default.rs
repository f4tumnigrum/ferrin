//! The process-wide default registry.

use std::sync::Arc;
use std::sync::OnceLock;

use ferrin_spec::DynLanguageModel;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::ModelRef;

use super::ProviderRegistry;
use crate::error::Error;

static DEFAULT_REGISTRY: OnceLock<Arc<ProviderRegistry>> = OnceLock::new();

/// Installs the registry used to resolve model ids passed as strings.
///
/// # Errors
///
/// Returns [`Error::InvalidArgument`] when a default registry is already set.
pub fn set_default_registry(registry: Arc<ProviderRegistry>) -> Result<(), Error> {
    DEFAULT_REGISTRY
        .set(registry)
        .map_err(|_| Error::invalid_argument("registry", "default registry is already set"))
}

/// The default registry, if one was installed.
#[must_use]
pub fn default_registry() -> Option<Arc<ProviderRegistry>> {
    DEFAULT_REGISTRY.get().cloned()
}

/// Resolves a model reference: already-resolved models are returned as is,
/// ids go through the default registry (`lookup` selects the modality).
pub(crate) fn resolve_model<D: ?Sized>(
    model: &ModelRef<D>,
    lookup: impl FnOnce(&ProviderRegistry, &str) -> Result<ModelRef<D>, Error>,
) -> Result<Arc<D>, Error> {
    if let Some(model) = model.model() {
        return Ok(Arc::clone(model));
    }
    let id = model.unresolved_id().unwrap_or_default();
    let Some(registry) = default_registry() else {
        return Err(Error::NoDefaultRegistry {
            model_id: id.to_owned(),
        });
    };
    lookup(&registry, id)?
        .into_model()
        .map_err(|id| Error::NoDefaultRegistry { model_id: id })
}

/// Resolves a language model reference.
pub(crate) fn resolve_language_model(
    model: &LanguageModelRef,
) -> Result<Arc<dyn DynLanguageModel>, Error> {
    resolve_model(model, ProviderRegistry::language_model)
}
