//! Provider registry and custom providers.
//!
//! A [`ProviderRegistry`] resolves `provider:model` ids across several
//! providers; [`custom_provider`] builds a provider from explicit model
//! instances; [`set_default_registry`] installs the registry used to resolve
//! bare model-id strings.

mod custom_provider;
pub(crate) mod default;
mod provider_registry;

pub use custom_provider::CustomProvider;
pub use custom_provider::CustomProviderBuilder;
pub use custom_provider::custom_provider;
pub use default::default_registry;
pub(crate) use default::resolve_language_model;
pub use default::set_default_registry;
pub use provider_registry::ProviderRegistry;
pub use provider_registry::ProviderRegistryBuilder;
pub use provider_registry::create_provider_registry;
