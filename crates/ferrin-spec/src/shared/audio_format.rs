//! Audio format descriptor for streaming audio inputs and outputs.

use serde::Deserialize;
use serde::Serialize;

/// Audio encoding and sample rate.
///
/// `kind` is a provider-understood format name such as `pcm16`, `g711_ulaw`
/// or `mp3`; `rate` is the sample rate in hertz when applicable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioFormat {
    /// Format name.
    #[serde(rename = "type")]
    pub kind: String,
    /// Sample rate in hertz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<u32>,
}

impl AudioFormat {
    /// Creates a format without a sample rate.
    #[must_use]
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            rate: None,
        }
    }

    /// Creates a format with a sample rate.
    #[must_use]
    pub fn with_rate(kind: impl Into<String>, rate: u32) -> Self {
        Self {
            kind: kind.into(),
            rate: Some(rate),
        }
    }
}
