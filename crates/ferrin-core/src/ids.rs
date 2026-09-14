//! Identifier generation for calls and approvals.

use std::sync::Arc;

pub use ferrin_provider_util::IdGenerator;
pub use ferrin_provider_util::PrefixedIdGenerator;

/// The default generator: 16 alphanumeric characters, no prefix.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultIdGenerator;

impl IdGenerator for DefaultIdGenerator {
    fn generate(&self) -> String {
        ferrin_provider_util::ids::generate_id()
    }
}

/// Returns the default generator as a shared trait object.
#[must_use]
pub fn default_id_generator() -> Arc<dyn IdGenerator> {
    Arc::new(DefaultIdGenerator)
}
