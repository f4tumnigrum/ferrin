//! Shared plumbing of the non-text modalities: request options, the
//! total timeout, User-Agent and provider metadata merging.

use std::future::Future;
use std::time::Duration;

use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ProviderOptions;
use tokio_util::sync::CancellationToken;

use crate::USER_AGENT;
use crate::cancel::CallCancellation;
use crate::error::Error;
use crate::retry::RetryPolicy;
use crate::telemetry::TelemetryOptions;
use crate::timeout::TimeoutScope;

/// Options every modality call accepts.
#[derive(Debug, Clone)]
pub(crate) struct ModalityOptions {
    pub(crate) headers: Headers,
    pub(crate) provider_options: ProviderOptions,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) cancellation: CancellationToken,
    pub(crate) timeout: Option<Duration>,
    pub(crate) telemetry: TelemetryOptions,
}

impl Default for ModalityOptions {
    fn default() -> Self {
        Self {
            headers: Headers::new(),
            provider_options: ProviderOptions::new(),
            retry_policy: RetryPolicy::default(),
            cancellation: CancellationToken::new(),
            timeout: None,
            telemetry: TelemetryOptions::default(),
        }
    }
}

impl ModalityOptions {
    /// The request headers with the core User-Agent product appended.
    pub(crate) fn request_headers(&self) -> Headers {
        self.headers.clone().with_user_agent_suffix([USER_AGENT])
    }

    /// Runs `operation` with the options and a child cancellation token
    /// under the optional total timeout; cancellation caused by the timeout
    /// is reported as [`Error::Timeout`].
    pub(crate) async fn run<T, F, Fut>(self, operation: F) -> Result<T, Error>
    where
        F: FnOnce(Self, CancellationToken) -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        let cancellation = CallCancellation::new(&self.cancellation);
        let timeout = self.timeout;
        let token = cancellation.token().clone();
        cancellation
            .with_timeout(TimeoutScope::Total, timeout, operation(self, token))
            .await
            .map_err(|error| cancellation.map_error(error))
    }
}

/// Adds the shared option setters to a builder with a `base` field.
macro_rules! impl_modality_builder {
    ($ty:ident $(< $($generic:ident),+ >)?) => {
        $crate::modality::impl_modality_builder!(@common $ty $(< $($generic),+ >)?);
        impl$(<$($generic),+>)? $ty$(<$($generic),+>)? {
            /// Sets the retry policy (default: two retries).
            #[must_use]
            pub fn retry(mut self, policy: $crate::retry::RetryPolicy) -> Self {
                self.base.retry_policy = policy;
                self
            }

            /// Sets the number of retries of the default policy.
            #[must_use]
            pub fn max_retries(mut self, max_retries: u32) -> Self {
                self.base.retry_policy.max_retries = max_retries;
                self
            }
        }
    };
    (@no_retry $ty:ident $(< $($generic:ident),+ >)?) => {
        $crate::modality::impl_modality_builder!(@common $ty $(< $($generic),+ >)?);
    };
    (@common $ty:ident $(< $($generic:ident),+ >)?) => {
        impl$(<$($generic),+>)? $ty$(<$($generic),+>)? {
            /// Adds request headers.
            #[must_use]
            pub fn headers(mut self, headers: ::ferrin_spec::Headers) -> Self {
                self.base.headers.merge(&headers);
                self
            }

            /// Adds one request header; invalid names or values are ignored.
            #[must_use]
            pub fn header(mut self, name: &str, value: &str) -> Self {
                let headers = ::std::mem::take(&mut self.base.headers);
                self.base.headers = headers.with(name, value);
                self
            }

            /// Sets the provider-specific options.
            #[must_use]
            pub fn provider_options(mut self, options: ::ferrin_spec::ProviderOptions) -> Self {
                self.base.provider_options = options;
                self
            }

            /// Sets the options of one provider.
            #[must_use]
            pub fn provider_option(
                mut self,
                provider: impl Into<String>,
                options: ::ferrin_spec::JsonObject,
            ) -> Self {
                self.base.provider_options.insert(provider.into(), options);
                self
            }

            /// Sets the cancellation token.
            #[must_use]
            pub fn cancellation(mut self, token: ::tokio_util::sync::CancellationToken) -> Self {
                self.base.cancellation = token;
                self
            }

            /// Sets the total timeout of the call.
            #[must_use]
            pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.base.timeout = Some(timeout);
                self
            }

            /// Sets the telemetry options.
            #[must_use]
            pub fn telemetry(mut self, options: $crate::telemetry::TelemetryOptions) -> Self {
                self.base.telemetry = options;
                self
            }
        }
    };
}
pub(crate) use impl_modality_builder;

/// Merges `source` into `target` provider by provider: objects are merged
/// key by key (later values win) and arrays under the same key are
/// concatenated, so per-item lists such as `images` stay aligned with the
/// combined results.
pub(crate) fn merge_provider_metadata(target: &mut ProviderMetadata, source: &ProviderMetadata) {
    for (provider, metadata) in source {
        let entry = target.entry(provider.clone()).or_default();
        for (key, value) in metadata {
            match (entry.get_mut(key), value) {
                (Some(JsonValue::Array(existing)), JsonValue::Array(incoming)) => {
                    existing.extend(incoming.iter().cloned());
                }
                _ => {
                    entry.insert(key.clone(), value.clone());
                }
            }
        }
    }
}

/// Merges optional provider metadata into an accumulator.
pub(crate) fn accumulate_provider_metadata(
    target: &mut Option<ProviderMetadata>,
    source: Option<&ProviderMetadata>,
) {
    if let Some(source) = source {
        merge_provider_metadata(target.get_or_insert_with(ProviderMetadata::new), source);
    }
}

/// Adds two optional counters; `None` counts as absent, not as zero.
pub(crate) fn add_optional(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}
