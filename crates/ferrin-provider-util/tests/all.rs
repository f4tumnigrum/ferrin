//! Single test binary aggregating the `suite/` modules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may panic on unexpected values"
)]

mod suite;
