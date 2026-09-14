//! Configuration of the Streamable HTTP transport.

use std::sync::Arc;
use std::time::Duration;

use ferrin_provider_util::http::SharedTransport;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::sse::DEFAULT_MAX_EVENT_BYTES;
use ferrin_spec::Headers;
use url::Url;

use super::common::DEFAULT_MAX_RESPONSE_BYTES;

/// Hook receiving a session id (`None` when the session was cleared).
pub type SessionHook = Arc<dyn Fn(Option<&str>) + Send + Sync>;

/// How `3xx` responses are handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RedirectMode {
    /// Redirects fail the request (default).
    #[default]
    Error,
    /// `307`/`308` redirects to the same origin are followed, up to the URL
    /// policy's redirect limit.
    Follow,
}

/// Reconnection of the legacy server-to-client event stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconnectionOptions {
    /// Delay before the first reconnection attempt.
    pub initial_delay: Duration,
    /// Upper bound of the delay.
    pub max_delay: Duration,
    /// Growth factor applied per attempt (`1.5` doubles every ~1.7 attempts).
    pub backoff_factor: f64,
    /// Maximum number of reconnection attempts.
    pub max_retries: u32,
}

impl Default for ReconnectionOptions {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_millis(30_000),
            backoff_factor: 1.5,
            max_retries: 2,
        }
    }
}

impl ReconnectionOptions {
    /// Delay before reconnection attempt `attempt` (zero-based).
    #[must_use]
    pub fn delay(&self, attempt: u32) -> Duration {
        let factor = self
            .backoff_factor
            .powi(i32::try_from(attempt).unwrap_or(i32::MAX));
        let seconds = self.initial_delay.as_secs_f64() * factor;
        Duration::try_from_secs_f64(seconds.max(0.0))
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }
}

/// Configuration of [`HttpTransport`](super::HttpTransport).
#[derive(Clone)]
#[non_exhaustive]
pub struct HttpTransportConfig {
    /// Endpoint URL.
    pub url: Url,
    /// Headers sent with every request.
    pub headers: Headers,
    /// OAuth provider used for `401` responses.
    #[cfg(feature = "oauth")]
    pub auth_provider: Option<Arc<dyn crate::oauth::OAuthClientProvider>>,
    /// Redirect handling (default [`RedirectMode::Error`]).
    pub redirect: RedirectMode,
    /// Session id to resume (legacy sessions).
    pub initial_session_id: Option<String>,
    /// Called when the server assigns or changes the session id.
    pub on_session_id_change: Option<SessionHook>,
    /// Called when the server reports the session as expired (`404`).
    pub on_session_expired: Option<SessionHook>,
    /// Whether `close` sends `DELETE` to terminate a legacy session
    /// (default `true`).
    pub terminate_session_on_close: bool,
    /// Secure URL policy the endpoint must satisfy (default: HTTPS only,
    /// public networks only).
    pub url_policy: UrlPolicy,
    /// Maximum size of a JSON response body (default 16 MiB).
    pub max_response_bytes: u64,
    /// Maximum size of one SSE event (default 16 MiB).
    pub max_event_bytes: usize,
    /// Reconnection of the legacy inbound event stream.
    pub reconnection: ReconnectionOptions,
    /// HTTP client (default: the shared reqwest transport).
    pub transport: Option<SharedTransport>,
}

impl std::fmt::Debug for HttpTransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("HttpTransportConfig");
        debug
            .field("url", &self.url)
            .field("headers", &self.headers.masked())
            .field("redirect", &self.redirect)
            .field(
                "initial_session_id",
                &self.initial_session_id.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "terminate_session_on_close",
                &self.terminate_session_on_close,
            )
            .field("url_policy", &self.url_policy)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_event_bytes", &self.max_event_bytes)
            .field("reconnection", &self.reconnection);
        #[cfg(feature = "oauth")]
        debug.field("auth_provider", &self.auth_provider.is_some());
        debug.finish_non_exhaustive()
    }
}

impl HttpTransportConfig {
    /// Default configuration for `url`.
    #[must_use]
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: Headers::new(),
            #[cfg(feature = "oauth")]
            auth_provider: None,
            redirect: RedirectMode::Error,
            initial_session_id: None,
            on_session_id_change: None,
            on_session_expired: None,
            terminate_session_on_close: true,
            url_policy: UrlPolicy::new(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            reconnection: ReconnectionOptions::default(),
            transport: None,
        }
    }

    /// Sets the headers sent with every request.
    #[must_use]
    pub fn headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Sets the OAuth provider.
    #[cfg(feature = "oauth")]
    #[must_use]
    pub fn auth_provider(mut self, provider: Arc<dyn crate::oauth::OAuthClientProvider>) -> Self {
        self.auth_provider = Some(provider);
        self
    }

    /// Sets the redirect mode.
    #[must_use]
    pub fn redirect(mut self, mode: RedirectMode) -> Self {
        self.redirect = mode;
        self
    }

    /// Resumes a legacy session.
    #[must_use]
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.initial_session_id = Some(session_id.into());
        self
    }

    /// Sets the session-id-change hook.
    #[must_use]
    pub fn on_session_id_change(mut self, hook: SessionHook) -> Self {
        self.on_session_id_change = Some(hook);
        self
    }

    /// Sets the session-expired hook.
    #[must_use]
    pub fn on_session_expired(mut self, hook: SessionHook) -> Self {
        self.on_session_expired = Some(hook);
        self
    }

    /// Controls whether `close` terminates legacy sessions.
    #[must_use]
    pub fn terminate_session_on_close(mut self, terminate: bool) -> Self {
        self.terminate_session_on_close = terminate;
        self
    }

    /// Sets the secure URL policy.
    #[must_use]
    pub fn url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }

    /// Sets the response body limit.
    #[must_use]
    pub fn max_response_bytes(mut self, bytes: u64) -> Self {
        self.max_response_bytes = bytes;
        self
    }

    /// Sets the reconnection options of the inbound stream.
    #[must_use]
    pub fn reconnection(mut self, options: ReconnectionOptions) -> Self {
        self.reconnection = options;
        self
    }

    /// Sets the HTTP client.
    #[must_use]
    pub fn transport(mut self, transport: SharedTransport) -> Self {
        self.transport = Some(transport);
        self
    }
}
