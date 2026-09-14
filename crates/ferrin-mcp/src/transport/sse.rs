//! Legacy HTTP+SSE transport (protocol version 2024-11-05).
//!
//! The client opens a `GET` event stream; the server announces the `POST`
//! endpoint in the first `endpoint` event and delivers every message on the
//! stream.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::SharedTransport;
use ferrin_provider_util::http::default_transport;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::sse::DEFAULT_MAX_EVENT_BYTES;
use ferrin_spec::Headers;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use http::Method;
use http::StatusCode;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::CloseOptions;
use super::EventChannel;
use super::McpTransport;
use super::SendOptions;
use super::TransportCapabilities;
use super::TransportEvent;
use super::common::AuthOutcome;
use super::common::Authenticator;
use super::common::DEFAULT_MAX_RESPONSE_BYTES;
use super::common::body_text;
use super::common::common_headers;
use super::common::emit_message_event;
use super::common::execute;
use super::common::pump_sse;
use super::common::status_error;
use super::common::validate;
use super::lock;
use super::with_cancellation;
use crate::error::McpError;
use crate::protocol::JsonRpcMessage;

/// Configuration of [`SseTransport`].
#[derive(Clone)]
#[non_exhaustive]
pub struct SseTransportConfig {
    /// URL of the event stream.
    pub url: Url,
    /// Headers sent with every request.
    pub headers: Headers,
    /// OAuth provider used for `401` responses.
    #[cfg(feature = "oauth")]
    pub auth_provider: Option<Arc<dyn crate::oauth::OAuthClientProvider>>,
    /// Secure URL policy the stream and endpoint URLs must satisfy.
    pub url_policy: UrlPolicy,
    /// Maximum size of an error response body (default 16 MiB).
    pub max_response_bytes: u64,
    /// Maximum size of one SSE event (default 16 MiB).
    pub max_event_bytes: usize,
    /// HTTP client (default: the shared reqwest transport).
    pub transport: Option<SharedTransport>,
}

impl std::fmt::Debug for SseTransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("SseTransportConfig");
        debug
            .field("url", &self.url)
            .field("headers", &self.headers.masked())
            .field("url_policy", &self.url_policy)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_event_bytes", &self.max_event_bytes);
        #[cfg(feature = "oauth")]
        debug.field("auth_provider", &self.auth_provider.is_some());
        debug.finish_non_exhaustive()
    }
}

impl SseTransportConfig {
    /// Default configuration for `url`.
    #[must_use]
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: Headers::new(),
            #[cfg(feature = "oauth")]
            auth_provider: None,
            url_policy: UrlPolicy::new(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
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

    /// Sets the secure URL policy.
    #[must_use]
    pub fn url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }

    /// Sets the HTTP client.
    #[must_use]
    pub fn transport(mut self, transport: SharedTransport) -> Self {
        self.transport = Some(transport);
        self
    }
}

#[derive(Debug, Default)]
struct State {
    started: bool,
    closed: bool,
    pinned: Vec<SocketAddr>,
    protocol_version: Option<String>,
    endpoint: Option<Url>,
}

struct Inner {
    config: SseTransportConfig,
    http: SharedTransport,
    auth: Authenticator,
    state: Mutex<State>,
    events: EventChannel,
    cancellation: CancellationToken,
    tasks: Mutex<JoinSet<()>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SseTransport")
            .field("config", &self.config)
            .field("state", &lock(&self.state))
            .finish_non_exhaustive()
    }
}

impl Inner {
    async fn headers(&self, base: &[(&str, &str)]) -> Headers {
        let mut headers = common_headers(&self.config.headers, base, &self.auth).await;
        if let Some(version) = lock(&self.state).protocol_version.clone() {
            let _ = headers.insert("mcp-protocol-version", &version);
        }
        headers
    }

    fn request(&self, method: Method, url: Url, headers: Headers) -> HttpRequest {
        let pinned = lock(&self.state).pinned.clone();
        HttpRequest::new(method, url)
            .with_headers(headers)
            .with_cancellation(self.cancellation.child_token())
            .with_pinned_addresses(pinned)
    }

    /// Resolves the `endpoint` event payload against the stream URL.
    fn set_endpoint(&self, data: &str) -> Result<(), McpError> {
        let endpoint = self
            .config
            .url
            .join(data.trim())
            .map_err(|error| McpError::protocol(format!("invalid endpoint event: {error}")))?;
        if endpoint.origin() != self.config.url.origin() {
            return Err(McpError::protocol(format!(
                "endpoint origin does not match connection origin: {}",
                endpoint.origin().ascii_serialization()
            )));
        }
        lock(&self.state).endpoint = Some(endpoint);
        Ok(())
    }

    async fn open_stream(self: &Arc<Self>) -> Result<(), McpError> {
        let mut authenticated = false;
        let url = self.config.url.clone();
        let response = loop {
            let headers = self.headers(&[("accept", "text/event-stream")]).await;
            let request = self.request(Method::GET, url.clone(), headers);
            let response = execute(self.http.as_ref(), request).await?;
            if response.status == StatusCode::UNAUTHORIZED {
                if self.auth.enabled() && !authenticated {
                    authenticated = true;
                    match self
                        .auth
                        .authorize(self.http.as_ref(), &url, &response.headers)
                        .await?
                    {
                        AuthOutcome::Authorized => continue,
                        AuthOutcome::Redirected => return Err(McpError::Unauthorized),
                    }
                }
                return Err(McpError::Unauthorized);
            }
            if !response.status.is_success() {
                let status = response.status;
                let text = body_text(response, self.config.max_response_bytes, &url)
                    .await
                    .ok();
                return Err(status_error(
                    "failed to open the event stream",
                    status,
                    &url,
                    text,
                ));
            }
            break response;
        };
        let (ready_sender, ready) = tokio::sync::oneshot::channel::<Result<(), McpError>>();
        let inner = Arc::clone(self);
        let cancellation = self.cancellation.child_token();
        let max_event_bytes = self.config.max_event_bytes;
        let mut ready_sender = Some(ready_sender);
        let stream_task = async move {
            let result = pump_sse(response, max_event_bytes, &cancellation, |event| {
                if event.event.as_deref() == Some("endpoint") {
                    let outcome = inner.set_endpoint(&event.data);
                    match ready_sender.take() {
                        Some(sender) => {
                            let _ = sender.send(outcome);
                        }
                        None => {
                            if let Err(error) = outcome {
                                inner.events.emit(TransportEvent::Error(error));
                            }
                        }
                    }
                } else {
                    emit_message_event(&event, &inner.events);
                }
            })
            .await;
            let failure = match result {
                Ok(false) => return,
                Ok(true) => McpError::transport("event stream ended"),
                Err(error) => error,
            };
            match ready_sender.take() {
                Some(sender) => {
                    let _ = sender.send(Err(failure));
                }
                None => inner.events.emit(TransportEvent::Error(failure)),
            }
            lock(&inner.state).closed = true;
            inner.events.emit(TransportEvent::Closed);
        };
        lock(&self.tasks).spawn(stream_task);
        ready.await.unwrap_or_else(|_| {
            Err(McpError::transport(
                "event stream closed before the endpoint event",
            ))
        })
    }

    async fn post(&self, message: JsonRpcMessage) -> Result<(), McpError> {
        let endpoint = {
            let state = lock(&self.state);
            if state.closed {
                return Err(McpError::Closed);
            }
            state
                .endpoint
                .clone()
                .ok_or_else(|| McpError::transport("transport has not been started"))?
        };
        let body = Bytes::from(message.to_json_string());
        let mut authenticated = false;
        loop {
            let headers = self.headers(&[("content-type", "application/json")]).await;
            let request = self
                .request(Method::POST, endpoint.clone(), headers)
                .with_body(RequestBody::json(body.clone()));
            let response = execute(self.http.as_ref(), request).await?;
            let status = response.status;
            if status == StatusCode::UNAUTHORIZED {
                if self.auth.enabled() && !authenticated {
                    authenticated = true;
                    match self
                        .auth
                        .authorize(self.http.as_ref(), &endpoint, &response.headers)
                        .await?
                    {
                        AuthOutcome::Authorized => continue,
                        AuthOutcome::Redirected => return Err(McpError::Unauthorized),
                    }
                }
                return Err(McpError::Unauthorized);
            }
            if !status.is_success() {
                let text = body_text(response, self.config.max_response_bytes, &endpoint)
                    .await
                    .ok();
                return Err(status_error(
                    "failed to post message",
                    status,
                    &endpoint,
                    text,
                ));
            }
            return Ok(());
        }
    }
}

/// Legacy HTTP+SSE transport.
#[derive(Debug)]
pub struct SseTransport {
    inner: Arc<Inner>,
}

impl SseTransport {
    /// Creates the transport.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Transport`] when no HTTP client was configured and
    /// the default one cannot be built.
    pub fn new(config: SseTransportConfig) -> Result<Self, McpError> {
        let http = match &config.transport {
            Some(transport) => Arc::clone(transport),
            None => {
                default_transport().map_err(|error| McpError::from_transport(error, &config.url))?
            }
        };
        #[cfg(feature = "oauth")]
        let auth = Authenticator::new(config.auth_provider.clone(), config.url_policy.clone());
        #[cfg(not(feature = "oauth"))]
        let auth = Authenticator::new();
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                http,
                auth,
                state: Mutex::new(State::default()),
                events: EventChannel::new(),
                cancellation: CancellationToken::new(),
                tasks: Mutex::new(JoinSet::new()),
            }),
        })
    }

    /// The `POST` endpoint announced by the server.
    #[must_use]
    pub fn endpoint(&self) -> Option<Url> {
        lock(&self.inner.state).endpoint.clone()
    }
}

impl McpTransport for SseTransport {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            if lock(&inner.state).started {
                return Err(McpError::transport("transport is already started"));
            }
            let pinned = validate(&inner.config.url, &inner.config.url_policy).await?;
            {
                let mut state = lock(&inner.state);
                state.pinned = pinned;
                state.started = true;
            }
            inner.open_stream().await
        })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            with_cancellation(options.cancellation.as_ref(), self.inner.post(message)).await
        })
    }

    fn incoming(&self) -> BoxStream<'static, TransportEvent> {
        self.inner.events.take()
    }

    fn close(&self, _options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            {
                let mut state = lock(&inner.state);
                if state.closed {
                    return Ok(());
                }
                state.closed = true;
            }
            inner.cancellation.cancel();
            lock(&inner.tasks).abort_all();
            inner.events.emit(TransportEvent::Closed);
            Ok(())
        })
    }

    fn protocol_version(&self) -> Option<String> {
        lock(&self.inner.state).protocol_version.clone()
    }

    fn set_protocol_version(&self, version: Option<&str>) {
        lock(&self.inner.state).protocol_version = version.map(str::to_owned);
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities::default()
    }
}
