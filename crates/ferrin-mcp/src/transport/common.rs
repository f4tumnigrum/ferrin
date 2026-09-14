//! Helpers shared by the HTTP-based transports: request execution, bounded
//! body reading, SSE pumping and the optional OAuth hook.

#[cfg(feature = "oauth")]
use std::sync::Arc;

use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::read_body;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::secure_url::validate_url;
use ferrin_provider_util::sse::SseEvent;
use ferrin_provider_util::sse::decode_stream;
use ferrin_spec::Headers;
use futures_util::StreamExt;
use http::StatusCode;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::EventChannel;
use super::TransportEvent;
use crate::error::McpError;
use crate::protocol::JsonRpcMessage;

/// `user-agent` suffix of every HTTP request.
pub(crate) const USER_AGENT: &str = concat!("ferrin-mcp/", env!("CARGO_PKG_VERSION"));

/// Default response body limit (16 MiB).
pub(crate) const DEFAULT_MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

/// Outcome of the OAuth hook after a `401`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthOutcome {
    /// Tokens are available; retry the request.
    #[cfg_attr(
        not(feature = "oauth"),
        allow(dead_code, reason = "only produced by the oauth flow")
    )]
    Authorized,
    /// The user must complete authorization in a browser.
    Redirected,
}

/// OAuth integration point of the HTTP transports.
pub(crate) struct Authenticator {
    #[cfg(feature = "oauth")]
    provider: Option<Arc<dyn crate::oauth::OAuthClientProvider>>,
    /// Whether an authorization flow is running.
    #[cfg(feature = "oauth")]
    in_flight: std::sync::Mutex<bool>,
    /// Bumped when a flow finishes; waiters subscribe to it.
    #[cfg(feature = "oauth")]
    generation: tokio::sync::watch::Sender<u64>,
    #[cfg(feature = "oauth")]
    policy: UrlPolicy,
}

impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator")
            .field("enabled", &self.enabled())
            .finish()
    }
}

impl Authenticator {
    #[cfg(feature = "oauth")]
    pub(crate) fn new(
        provider: Option<Arc<dyn crate::oauth::OAuthClientProvider>>,
        policy: UrlPolicy,
    ) -> Self {
        Self {
            provider,
            in_flight: std::sync::Mutex::new(false),
            generation: tokio::sync::watch::Sender::new(0),
            policy,
        }
    }

    #[cfg(not(feature = "oauth"))]
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Whether an OAuth provider is configured.
    pub(crate) fn enabled(&self) -> bool {
        #[cfg(feature = "oauth")]
        {
            self.provider.is_some()
        }
        #[cfg(not(feature = "oauth"))]
        {
            false
        }
    }

    /// Current bearer token, if any; storage failures are logged and treated
    /// as "no token".
    pub(crate) async fn bearer_token(&self) -> Option<String> {
        #[cfg(feature = "oauth")]
        {
            let provider = self.provider.as_ref()?;
            match provider.tokens().await {
                Ok(tokens) => tokens.map(|tokens| {
                    secrecy::ExposeSecret::expose_secret(&tokens.access_token).to_owned()
                }),
                Err(error) => {
                    tracing::warn!(error = %error, "failed to load OAuth tokens");
                    None
                }
            }
        }
        #[cfg(not(feature = "oauth"))]
        {
            None
        }
    }

    /// Runs one authorization flow for a `401` response (concurrent callers
    /// wait for the running flow).
    #[allow(
        unused_variables,
        reason = "parameters are unused without the oauth feature"
    )]
    pub(crate) async fn authorize(
        &self,
        http: &dyn HttpTransport,
        server_url: &Url,
        response_headers: &Headers,
    ) -> Result<AuthOutcome, McpError> {
        #[cfg(feature = "oauth")]
        {
            let Some(provider) = &self.provider else {
                return Ok(AuthOutcome::Redirected);
            };
            let params = crate::oauth::extract_www_authenticate_params(response_headers);
            let options = crate::oauth::AuthOptions::new(server_url.clone())
                .resource_metadata_url(params.resource_metadata_url)
                .scope(params.scope)
                .url_policy(self.policy.clone());
            // One flow at a time: concurrent 401s wait for the running flow
            // and then retry with whatever tokens it stored.
            let seen = {
                let mut in_flight = super::lock(&self.in_flight);
                if *in_flight {
                    Some(*self.generation.borrow())
                } else {
                    *in_flight = true;
                    None
                }
            };
            if let Some(seen) = seen {
                let mut generation = self.generation.subscribe();
                let _ = generation.wait_for(|current| *current > seen).await;
                return Ok(if self.bearer_token().await.is_some() {
                    AuthOutcome::Authorized
                } else {
                    AuthOutcome::Redirected
                });
            }
            let outcome = crate::oauth::auth(provider.as_ref(), http, options).await;
            *super::lock(&self.in_flight) = false;
            self.generation.send_modify(|current| *current += 1);
            match outcome? {
                crate::oauth::AuthResult::Authorized => Ok(AuthOutcome::Authorized),
                crate::oauth::AuthResult::Redirect => Ok(AuthOutcome::Redirected),
                #[allow(unreachable_patterns, reason = "AuthResult is non-exhaustive")]
                _ => Ok(AuthOutcome::Redirected),
            }
        }
        #[cfg(not(feature = "oauth"))]
        {
            Ok(AuthOutcome::Redirected)
        }
    }
}

/// Base headers of every request: configured headers, `base`, the bearer
/// token and the user-agent suffix.
pub(crate) async fn common_headers(
    configured: &Headers,
    base: &[(&str, &str)],
    auth: &Authenticator,
) -> Headers {
    let mut headers = configured.clone();
    for (name, value) in base {
        let _ = headers.insert(name, value);
    }
    if let Some(token) = auth.bearer_token().await {
        let _ = headers.insert("authorization", &format!("Bearer {token}"));
    }
    headers.with_user_agent_suffix([USER_AGENT])
}

/// Validates `url` against `policy` and returns the pinned addresses.
pub(crate) async fn validate(
    url: &Url,
    policy: &UrlPolicy,
) -> Result<Vec<std::net::SocketAddr>, McpError> {
    Ok(validate_url(url, policy).await?.addresses)
}

/// Executes `request`, mapping transport failures.
pub(crate) async fn execute(
    http: &dyn HttpTransport,
    request: HttpRequest,
) -> Result<HttpResponse, McpError> {
    let url = request.url.clone();
    http.execute(request)
        .await
        .map_err(|error| McpError::from_transport(error, &url))
}

/// Reads a bounded response body as text.
pub(crate) async fn body_text(
    response: HttpResponse,
    max_bytes: u64,
    url: &Url,
) -> Result<String, McpError> {
    let bytes = read_body(&response.headers, response.body, max_bytes)
        .await
        .map_err(|error| McpError::from_transport(error, url))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether the `content-type` header names `expected`.
pub(crate) fn content_type_is(headers: &Headers, expected: &str) -> bool {
    headers
        .get_str("content-type")
        .is_some_and(|value| value.to_ascii_lowercase().contains(expected))
}

/// Error for a non-success status, carrying the response text.
pub(crate) fn status_error(
    prefix: &str,
    status: StatusCode,
    url: &Url,
    body: Option<String>,
) -> McpError {
    let message = match &body {
        Some(text) if !text.is_empty() => format!("{prefix} (HTTP {status}): {text}"),
        _ => format!("{prefix} (HTTP {status})"),
    };
    McpError::http_status(message, status, url, body)
}

/// Parses an SSE `message` event into JSON-RPC messages and emits them.
pub(crate) fn emit_message_event(event: &SseEvent, events: &EventChannel) {
    if event.event.as_deref().is_some_and(|name| name != "message") {
        return;
    }
    match JsonRpcMessage::parse_one_or_many(&event.data) {
        Ok(messages) => {
            for message in messages {
                events.emit(TransportEvent::Message(message));
            }
        }
        Err(error) => events.emit(TransportEvent::Error(McpError::protocol(format!(
            "failed to parse message: {error}"
        )))),
    }
}

/// Reads SSE events from `response` until the stream ends or `cancellation`
/// fires, calling `on_event` for each one.
///
/// Returns `Ok(true)` when the stream ended normally, `Ok(false)` when it
/// was cancelled and an error when the body failed or exceeded the limit.
pub(crate) async fn pump_sse(
    response: HttpResponse,
    max_event_bytes: usize,
    cancellation: &CancellationToken,
    mut on_event: impl FnMut(SseEvent),
) -> Result<bool, McpError> {
    let mut stream = std::pin::pin!(decode_stream(response.body, max_event_bytes));
    loop {
        let next = tokio::select! {
            () = cancellation.cancelled() => return Ok(false),
            next = stream.next() => next,
        };
        match next {
            Some(Ok(event)) => on_event(event),
            Some(Err(error)) => {
                return Err(McpError::transport(format!("event stream failed: {error}")));
            }
            None => return Ok(true),
        }
    }
}

/// Resolves the same-origin target of a redirect response to `url`.
pub(crate) fn redirect_target(url: &Url, headers: &Headers) -> Result<Url, McpError> {
    let location = headers
        .get_str("location")
        .ok_or_else(|| McpError::transport("redirect response without a location header"))?;
    let target = url
        .join(location)
        .map_err(|error| McpError::transport(format!("invalid redirect location: {error}")))?;
    if target.origin() != url.origin() {
        return Err(McpError::transport(format!(
            "redirect to another origin is not allowed: {}",
            target.host_str().unwrap_or_default()
        )));
    }
    Ok(target)
}

/// Whether `status` is a redirect the transport may follow.
pub(crate) fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT
    )
}
