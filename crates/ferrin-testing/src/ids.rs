//! Deterministic identifiers for snapshot tests.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use ferrin_provider_util::IdGenerator;

/// Generates `<prefix>-0`, `<prefix>-1`, ... in call order.
#[derive(Debug)]
pub struct SequentialIdGenerator {
    prefix: String,
    next: AtomicU64,
}

impl SequentialIdGenerator {
    /// Creates a generator with `prefix`.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            next: AtomicU64::new(0),
        }
    }

    /// Number of ids generated so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.next.load(Ordering::SeqCst)
    }
}

impl Default for SequentialIdGenerator {
    fn default() -> Self {
        Self::new("id")
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn generate(&self) -> String {
        let index = self.next.fetch_add(1, Ordering::SeqCst);
        format!("{}-{index}", self.prefix)
    }
}
