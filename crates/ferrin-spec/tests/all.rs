//! Single test binary aggregating the `suite/` modules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code may panic on unexpected values"
)]

mod suite;
