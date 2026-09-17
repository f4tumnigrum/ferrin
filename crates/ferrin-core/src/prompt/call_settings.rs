//! Sampling settings shared by all text generation entry points.

use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ReasoningEffort;

use crate::error::Error;
use crate::middleware::builtin::merge_provider_options;

/// Sampling and request settings applied to every model call.
///
/// Rust types absorb most validation (`u32`, `f64`, `Vec<String>`); the
/// remaining runtime checks are `max_output_tokens >= 1` and finite floats.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallSettings {
    /// Maximum number of output tokens.
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Nucleus sampling.
    pub top_p: Option<f64>,
    /// Top-k sampling.
    pub top_k: Option<u32>,
    /// Presence penalty.
    pub presence_penalty: Option<f64>,
    /// Frequency penalty.
    pub frequency_penalty: Option<f64>,
    /// Stop sequences.
    pub stop_sequences: Option<Vec<String>>,
    /// Random seed.
    pub seed: Option<u64>,
    /// Reasoning effort.
    pub reasoning: ReasoningEffort,
    /// Extra request headers.
    pub headers: Headers,
    /// Provider-specific options.
    pub provider_options: ProviderOptions,
}

impl CallSettings {
    /// Checks the runtime invariants.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidArgument`] naming the offending setting.
    pub fn validate(&self) -> Result<(), Error> {
        if self.max_output_tokens == Some(0) {
            return Err(Error::invalid_argument(
                "max_output_tokens",
                "must be at least 1",
            ));
        }
        for (name, value) in [
            ("temperature", self.temperature),
            ("top_p", self.top_p),
            ("presence_penalty", self.presence_penalty),
            ("frequency_penalty", self.frequency_penalty),
        ] {
            if let Some(value) = value
                && !value.is_finite()
            {
                return Err(Error::invalid_argument(name, "must be a finite number"));
            }
        }
        Ok(())
    }

    /// Copies the settings into `options`, merging headers and provider
    /// options.
    pub fn apply(&self, options: &mut CallOptions) {
        options.max_output_tokens = self.max_output_tokens;
        options.temperature = self.temperature;
        options.top_p = self.top_p;
        options.top_k = self.top_k;
        options.presence_penalty = self.presence_penalty;
        options.frequency_penalty = self.frequency_penalty;
        options.stop_sequences = self.stop_sequences.clone();
        options.seed = self.seed;
        options.reasoning = self.reasoning;
        options.headers.merge(&self.headers);
        options.provider_options =
            merge_provider_options(&options.provider_options, self.provider_options.clone());
    }

    /// Overlays the set fields of `other` onto `self` (used by
    /// `prepare_step` and default-settings middleware).
    pub fn merge(&mut self, other: &CallSettings) {
        if other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
        }
        if other.temperature.is_some() {
            self.temperature = other.temperature;
        }
        if other.top_p.is_some() {
            self.top_p = other.top_p;
        }
        if other.top_k.is_some() {
            self.top_k = other.top_k;
        }
        if other.presence_penalty.is_some() {
            self.presence_penalty = other.presence_penalty;
        }
        if other.frequency_penalty.is_some() {
            self.frequency_penalty = other.frequency_penalty;
        }
        if other.stop_sequences.is_some() {
            self.stop_sequences.clone_from(&other.stop_sequences);
        }
        if other.seed.is_some() {
            self.seed = other.seed;
        }
        if other.reasoning != ReasoningEffort::default() {
            self.reasoning = other.reasoning;
        }
        self.headers.merge(&other.headers);
        self.provider_options =
            merge_provider_options(&self.provider_options, other.provider_options.clone());
    }
}
