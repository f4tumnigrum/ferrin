//! Cache control breakpoints read from part and message options.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;

use crate::config::AnthropicConfig;
use crate::options::part_options;

/// Largest number of cache breakpoints the API accepts per request.
pub const MAX_CACHE_BREAKPOINTS: usize = 4;

/// Reads `cacheControl` (or `cache_control`) from an option object.
#[must_use]
pub fn cache_control_value(options: Option<&JsonObject>) -> Option<JsonValue> {
    let options = options?;
    options
        .get("cacheControl")
        .or_else(|| options.get("cache_control"))
        .filter(|value| !value.is_null())
        .cloned()
}

/// Counts breakpoints and rejects cache control where the API forbids it.
#[derive(Debug, Default)]
pub struct CacheControlValidator {
    breakpoints: usize,
    warnings: Vec<Warning>,
}

impl CacheControlValidator {
    /// Creates an empty validator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cache control of a part or message, if any, or `None`
    /// with a warning when `can_cache` is `false` or the breakpoint limit is
    /// exceeded.
    pub fn get(
        &mut self,
        config: &AnthropicConfig,
        provider_options: Option<&ProviderOptions>,
        context: &str,
        can_cache: bool,
    ) -> Option<JsonValue> {
        let value = cache_control_value(part_options(config, provider_options))?;
        if !can_cache {
            self.warnings.push(Warning::unsupported_with_details(
                "cache_control on non-cacheable context",
                format!("cache_control cannot be set on {context}. It will be ignored."),
            ));
            return None;
        }
        self.breakpoints += 1;
        if self.breakpoints > MAX_CACHE_BREAKPOINTS {
            self.warnings.push(Warning::unsupported_with_details(
                "cacheControl breakpoint limit",
                format!(
                    "Maximum {MAX_CACHE_BREAKPOINTS} cache breakpoints exceeded (found {}). This breakpoint will be ignored.",
                    self.breakpoints
                ),
            ));
            return None;
        }
        Some(value)
    }

    /// Warnings collected so far.
    #[must_use]
    pub fn into_warnings(self) -> Vec<Warning> {
        self.warnings
    }
}
