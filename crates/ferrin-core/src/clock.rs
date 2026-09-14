//! Wall-clock source for response timestamps.
//!
//! Injecting a [`Clock`] lets tests produce deterministic
//! `response.timestamp` values without touching the system time.

use std::fmt;
use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;

/// Provides the current wall-clock time.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> DateTime<Utc>;
}

impl<F> Clock for F
where
    F: Fn() -> DateTime<Utc> + Send + Sync,
{
    fn now(&self) -> DateTime<Utc> {
        self()
    }
}

impl<T: Clock + ?Sized> Clock for Arc<T> {
    fn now(&self) -> DateTime<Utc> {
        (**self).now()
    }
}

impl fmt::Debug for dyn Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock(..)")
    }
}

/// The system clock (`chrono::Utc::now`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that always returns the same instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

/// Returns the system clock as a shared trait object.
#[must_use]
pub fn default_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}
