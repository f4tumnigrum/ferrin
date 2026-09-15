//! Retryability classification and `Retry-After` parsing.
//!
//! The retry loop itself lives in the core crate; this module provides the
//! header-level pieces adapters and the loop share.

use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use ferrin_spec::Headers;
use http::StatusCode;

/// Returns `true` for 408, 409, 429 and every 5xx status.
#[must_use]
pub fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}

/// Reads the delay requested by `retry-after-ms` (milliseconds) or
/// `retry-after` (seconds or HTTP date).
///
/// Returns `None` when neither header is present or parsable.
#[must_use]
pub fn retry_after(headers: &Headers) -> Option<Duration> {
    if let Some(value) = headers.get_str("retry-after-ms")
        && let Ok(millis) = value.trim().parse::<f64>()
        && let Ok(delay) = Duration::try_from_secs_f64(millis / 1000.0)
    {
        return Some(delay);
    }
    let value = headers.get_str("retry-after")?.trim();
    if let Ok(seconds) = value.parse::<f64>() {
        return Duration::try_from_secs_f64(seconds).ok();
    }
    let date = DateTime::parse_from_rfc2822(value).ok()?;
    let delay = date.with_timezone(&Utc) - Utc::now();
    delay.to_std().ok()
}

/// Applies the reference window: a `Retry-After` delay is used only when it
/// is at most `max` (default 60 s).
#[must_use]
pub fn retry_after_within(headers: &Headers, max: Duration) -> Option<Duration> {
    retry_after(headers).filter(|delay| *delay <= max)
}
