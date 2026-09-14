//! Token usage reported by language models.

use serde::Deserialize;
use serde::Serialize;

use crate::json::JsonObject;

/// Token usage of a single language model call or of several added calls.
///
/// Every counter is optional because providers report different subsets.
/// Standard fields that cannot be mapped are `None`; the provider's original
/// usage object is available in `raw`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input (prompt) token counts.
    #[serde(default)]
    pub input: InputTokens,
    /// Output (completion) token counts.
    #[serde(default)]
    pub output: OutputTokens,
    /// Provider-specific usage object as returned by the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<JsonObject>,
}

/// Input token counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputTokens {
    /// Total input tokens, including cached tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Input tokens that were neither read from nor written to a cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u64>,
    /// Input tokens read from a prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    /// Input tokens written to a prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
}

/// Output token counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTokens {
    /// Total output tokens, including reasoning tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Output tokens that are visible text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<u64>,
    /// Output tokens spent on reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

/// Adds two optional counters: `None + None = None`, `Some(a) + None = Some(a)`.
#[must_use]
pub fn add_token_counts(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
    }
}

impl Usage {
    /// Creates a usage with only the total input and output counts set.
    #[must_use]
    pub fn totals(input: u64, output: u64) -> Self {
        Self {
            input: InputTokens {
                total: Some(input),
                ..InputTokens::default()
            },
            output: OutputTokens {
                total: Some(output),
                ..OutputTokens::default()
            },
            raw: None,
        }
    }

    /// Adds two usages counter by counter.
    ///
    /// `None + None = None`; `Some(a) + None = Some(a)`. The `raw` object is
    /// dropped because provider payloads cannot be combined generically.
    #[must_use]
    pub fn add(&self, other: &Usage) -> Usage {
        Usage {
            input: InputTokens {
                total: add_token_counts(self.input.total, other.input.total),
                no_cache: add_token_counts(self.input.no_cache, other.input.no_cache),
                cache_read: add_token_counts(self.input.cache_read, other.input.cache_read),
                cache_write: add_token_counts(self.input.cache_write, other.input.cache_write),
            },
            output: OutputTokens {
                total: add_token_counts(self.output.total, other.output.total),
                text: add_token_counts(self.output.text, other.output.text),
                reasoning: add_token_counts(self.output.reasoning, other.output.reasoning),
            },
            raw: None,
        }
    }

    /// Total tokens: input total plus output total (`None` when both unknown).
    #[must_use]
    pub fn total_tokens(&self) -> Option<u64> {
        add_token_counts(self.input.total, self.output.total)
    }
}
