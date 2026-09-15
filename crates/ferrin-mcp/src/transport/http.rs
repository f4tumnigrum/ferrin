//! Streamable HTTP transport (protocol versions 2025-11-25 and 2026-07-28).
//!
//! Every message is a `POST` to the endpoint; responses arrive either as a
//! JSON body or as a `text/event-stream` body. Legacy sessions additionally
//! open a `GET` event stream for server-initiated messages.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::SharedTransport;
use ferrin_provider_util::http::default_transport;
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
use super::common::body_text;
use super::common::common_headers;
use super::common::content_type_is;
use super::common::emit_message_event;
use super::common::execute;
use super::common::is_redirect;
use super::common::pump_sse;
use super::common::redirect_target;
use super::common::status_error;
use super::common::validate;
use super::headers::encode_header_value;
use super::http_config::HttpTransportConfig;
use super::http_config::RedirectMode;
use super::http_stream::pump_response;
use super::lock;
use super::with_cancellation;
use crate::error::McpError;
use crate::protocol::JsonRpcMessage;
use crate::protocol::ProtocolEra;

fn is_initialize(message: &JsonRpcMessage) -> bool {
    message.method() == Some("initialize")
}

#[derive(Debug, Default)]
struct State {
    started: bool,
    closed: bool,
    pinned: Vec<SocketAddr>,
    session_id: Option<String>,
    protocol_version: Option<String>,
    last_event_id: Option<String>,
    inbound_started: bool,
}

struct Inner {
    config: HttpTransportConfig,
    http: SharedTransport,
    auth: Authenticator,
    state: Mutex<State>,
    events: EventChannel,
    cancellation: CancellationToken,
    tasks: Mutex<JoinSet<()>>,
}

impl Inner {
    fn era(&self) -> ProtocolEra {
        lock(&self.state)
            .protocol_version
            .as_deref()
            .map_or(ProtocolEra::Legacy, ProtocolEra::of_version)
    }

    fn set_session_id(&self, session_id: Option<&str>) {
        let changed = {
            let mut state = lock(&self.state);
            if state.session_id.as_deref() == session_id {
                false
            } else {
                state.session_id = session_id.map(str::to_owned);
                true
            }
        };
        if changed && let Some(hook) = &self.config.on_session_id_change {
            hook(session_id);
        }
    }

    fn expire_session(&self) {
        let expired = lock(&self.state).session_id.take();
        if let Some(hook) = &self.config.on_session_expired {
            hook(expired.as_deref());
        }
    }

    async fn base_headers(&self, include_session: bool, base: &[(&str, &str)]) -> Headers {
        let (protocol_version, session_id) = {
            let state = lock(&self.state);
            (state.protocol_version.clone(), state.session_id.clone())
        };
        let mut headers = common_headers(&self.config.headers, base, &self.auth).await;
        if let Some(version) = &protocol_version {
            let _ = headers.insert("mcp-protocol-version", version);
        }
        if include_session
            && !self.era().is_modern()
            && let Some(session_id) = &session_id
        {
            let _ = headers.insert("mcp-session-id", session_id);
        }
        headers
    }

    async fn post_headers(&self, message: &JsonRpcMessage, options: &SendOptions) -> Headers {
        let include_session = !is_initialize(message);
        let mut headers = self
            .base_headers(
                include_session,
                &[
                    ("content-type", "application/json"),
                    ("accept", "application/json, text/event-stream"),
                ],
            )
            .await;
        if self.era().is_modern() {
            if let Some(method) = message.method() {
                let _ = headers.insert("mcp-method", method);
            }
            if let Some(name) = mcp_name(message) {
                let _ = headers.insert("mcp-name", &encode_header_value(name));
            }
            headers.merge(&options.headers);
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

    fn spawn(self: &Arc<Self>, future: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = lock(&self.tasks);
        while tasks.try_join_next().is_some() {}
        tasks.spawn(future);
    }

    #[allow(clippy::too_many_lines, reason = "one linear response state machine")]
    async fn send_message(
        self: &Arc<Self>,
        message: JsonRpcMessage,
        options: &SendOptions,
    ) -> Result<(), McpError> {
        {
            let state = lock(&self.state);
            if state.closed {
                return Err(McpError::Closed);
            }
            if !state.started {
                return Err(McpError::transport("transport has not been started"));
            }
        }
        let body = Bytes::from(message.to_json_string());
        let mut url = self.config.url.clone();
        let mut authenticated = false;
        let mut redirects: u8 = 0;
        loop {
            let headers = self.post_headers(&message, options).await;
            let request = self
                .request(Method::POST, url.clone(), headers)
                .with_body(RequestBody::json(body.clone()));
            let response = execute(self.http.as_ref(), request).await?;
            if let Some(session_id) = response.headers.get_str("mcp-session-id") {
                self.set_session_id(Some(session_id));
            }
            let status = response.status;
            if status == StatusCode::UNAUTHORIZED {
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
            if is_redirect(status) && self.config.redirect == RedirectMode::Follow {
                redirects += 1;
                if redirects > self.config.url_policy.max_redirects {
                    return Err(McpError::transport("too many redirects"));
                }
                url = redirect_target(&url, &response.headers)?;
                continue;
            }
            if status == StatusCode::ACCEPTED {
                if message.method() == Some("notifications/initialized") && !self.era().is_modern()
                {
                    self.start_inbound();
                }
                return Ok(());
            }
            if !status.is_success() {
                let text = body_text(response, self.config.max_response_bytes, &url).await?;
                if let Ok(JsonRpcMessage::Error(mut error)) = JsonRpcMessage::parse(&text) {
                    if error.id.is_none() {
                        error.id = message.id().cloned();
                    }
                    self.events
                        .emit(TransportEvent::Message(JsonRpcMessage::Error(error)));
                    return Ok(());
                }
                if status == StatusCode::NOT_FOUND && lock(&self.state).session_id.is_some() {
                    self.expire_session();
                    return Err(status_error(
                        "session expired; reconnect to start a new session",
                        status,
                        &url,
                        Some(text),
                    ));
                }
                return Err(status_error("request failed", status, &url, Some(text)));
            }
            let JsonRpcMessage::Request(request) = &message else {
                return Ok(());
            };
            if content_type_is(&response.headers, "text/event-stream") {
                let inner = Arc::clone(self);
                let cancellation = self.cancellation.child_token();
                let max_event_bytes = self.config.max_event_bytes;
                let request_cancellation = options.cancellation.clone();
                let request_id = request.id.clone();
                self.spawn(async move {
                    let pump = pump_response(
                        response,
                        max_event_bytes,
                        &cancellation,
                        &inner.events,
                        &request_id,
                    );
                    let result = with_cancellation(request_cancellation.as_ref(), pump).await;
                    if let Err(error) = result {
                        if matches!(error, McpError::Cancelled) {
                            return;
                        }
                        inner.events.emit(TransportEvent::RequestError {
                            id: request_id,
                            error,
                        });
                    }
                });
                return Ok(());
            }
            if content_type_is(&response.headers, "application/json") {
                let text = body_text(response, self.config.max_response_bytes, &url).await?;
                for parsed in JsonRpcMessage::parse_one_or_many(&text)? {
                    self.events.emit(TransportEvent::Message(parsed));
                }
                return Ok(());
            }
            let content_type = response
                .headers
                .get_str("content-type")
                .unwrap_or_default()
                .to_owned();
            return Err(McpError::transport(format!(
                "unexpected content type \"{content_type}\" in MCP response"
            )));
        }
    }

    /// Starts the legacy server-to-client event stream once.
    fn start_inbound(self: &Arc<Self>) {
        {
            let mut state = lock(&self.state);
            if state.inbound_started || state.closed {
                return;
            }
            state.inbound_started = true;
        }
        let inner = Arc::clone(self);
        self.spawn(async move { inner.run_inbound().await });
    }

    async fn run_inbound(self: Arc<Self>) {
        let cancellation = self.cancellation.child_token();
        let mut attempt: u32 = 0;
        let mut authenticated = false;
        loop {
            let mut headers = self
                .base_headers(true, &[("accept", "text/event-stream")])
                .await;
            if let Some(last_event_id) = lock(&self.state).last_event_id.clone() {
                let _ = headers.insert("last-event-id", &last_event_id);
            }
            let url = self.config.url.clone();
            let request = self.request(Method::GET, url.clone(), headers);
            let response = match execute(self.http.as_ref(), request).await {
                Ok(response) => response,
                Err(McpError::Cancelled) => return,
                Err(error) => {
                    self.events.emit(TransportEvent::Error(error));
                    return;
                }
            };
            let status = response.status;
            if status == StatusCode::UNAUTHORIZED && self.auth.enabled() && !authenticated {
                authenticated = true;
                match self
                    .auth
                    .authorize(self.http.as_ref(), &url, &response.headers)
                    .await
                {
                    Ok(AuthOutcome::Authorized) => continue,
                    Ok(AuthOutcome::Redirected) => {
                        self.events
                            .emit(TransportEvent::Error(McpError::Unauthorized));
                        return;
                    }
                    Err(error) => {
                        self.events.emit(TransportEvent::Error(error));
                        return;
                    }
                }
            }
            if status == StatusCode::METHOD_NOT_ALLOWED {
                // The server does not offer a standalone event stream.
                return;
            }
            if !status.is_success() {
                let text = body_text(response, self.config.max_response_bytes, &url)
                    .await
                    .ok();
                self.events.emit(TransportEvent::Error(status_error(
                    "failed to open the server event stream",
                    status,
                    &url,
                    text,
                )));
                return;
            }
            let result = pump_sse(
                response,
                self.config.max_event_bytes,
                &cancellation,
                |event| {
                    if let Some(id) = &event.id {
                        lock(&self.state).last_event_id = Some(id.clone());
                    }
                    emit_message_event(&event, &self.events);
                },
            )
            .await;
            match result {
                Ok(false) => return,
                Ok(true) => return,
                Err(error) => {
                    let options = self.config.reconnection;
                    if attempt >= options.max_retries {
                        self.events.emit(TransportEvent::Error(error));
                        return;
                    }
                    let delay = options.delay(attempt);
                    attempt += 1;
                    tokio::select! {
                        () = cancellation.cancelled() => return,
                        () = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    async fn terminate_session(
        &self,
        session_id: &str,
        options: &CloseOptions,
    ) -> Result<(), McpError> {
        let mut headers = common_headers(&self.config.headers, &[], &self.auth).await;
        let _ = headers.insert("mcp-session-id", session_id);
        if let Some(version) = lock(&self.state).protocol_version.clone() {
            let _ = headers.insert("mcp-protocol-version", &version);
        }
        let url = self.config.url.clone();
        let pinned = lock(&self.state).pinned.clone();
        let request = HttpRequest::new(Method::DELETE, url.clone())
            .with_headers(headers)
            .with_pinned_addresses(pinned);
        let response = with_cancellation(
            options.cancellation.as_ref(),
            execute(self.http.as_ref(), request),
        )
        .await?;
        if response.status.is_success() || response.status == StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        let status = response.status;
        let text = body_text(response, self.config.max_response_bytes, &url)
            .await
            .ok();
        Err(status_error(
            "failed to terminate the session",
            status,
            &url,
            text,
        ))
    }
}

/// Method-specific `Mcp-Name` value.
fn mcp_name(message: &JsonRpcMessage) -> Option<&str> {
    let params = message.params()?;
    match message.method()? {
        "tools/call" | "prompts/get" => params.get("name")?.as_str(),
        "resources/read" => params.get("uri")?.as_str(),
        _ => None,
    }
}

/// Streamable HTTP transport.
#[derive(Debug)]
pub struct HttpTransport {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTransport")
            .field("config", &self.config)
            .field("state", &lock(&self.state))
            .finish_non_exhaustive()
    }
}

impl HttpTransport {
    /// Creates the transport.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Transport`] when no HTTP client was configured and
    /// the default one cannot be built.
    pub fn new(config: HttpTransportConfig) -> Result<Self, McpError> {
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
        let state = State {
            session_id: config.initial_session_id.clone(),
            ..State::default()
        };
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                http,
                auth,
                state: Mutex::new(state),
                events: EventChannel::new(),
                cancellation: CancellationToken::new(),
                tasks: Mutex::new(JoinSet::new()),
            }),
        })
    }

    /// Current session id (legacy sessions).
    #[must_use]
    pub fn session_id(&self) -> Option<String> {
        lock(&self.inner.state).session_id.clone()
    }

    /// Endpoint URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.inner.config.url
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        self.inner.cancellation.cancel();
        lock(&self.inner.tasks).abort_all();
    }
}

impl McpTransport for HttpTransport {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            if lock(&inner.state).started {
                return Err(McpError::transport("transport is already started"));
            }
            let pinned = validate(&inner.config.url, &inner.config.url_policy).await?;
            let mut state = lock(&inner.state);
            state.pinned = pinned;
            state.started = true;
            Ok(())
        })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let cancellation = options.cancellation.clone();
            with_cancellation(
                cancellation.as_ref(),
                self.inner.send_message(message, &options),
            )
            .await
        })
    }

    fn incoming(&self) -> BoxStream<'static, TransportEvent> {
        self.inner.events.take()
    }

    fn close(&self, options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            let (session_id, terminate) = {
                let mut state = lock(&inner.state);
                if state.closed {
                    return Ok(());
                }
                state.closed = true;
                let terminate = inner.config.terminate_session_on_close
                    && state.started
                    && !state
                        .protocol_version
                        .as_deref()
                        .is_some_and(|version| ProtocolEra::of_version(version).is_modern());
                (state.session_id.clone(), terminate)
            };
            inner.cancellation.cancel();
            lock(&inner.tasks).abort_all();
            let result = match (session_id, terminate) {
                (Some(session_id), true) => inner.terminate_session(&session_id, &options).await,
                _ => Ok(()),
            };
            inner.events.emit(TransportEvent::Closed);
            result
        })
    }

    fn protocol_version(&self) -> Option<String> {
        lock(&self.inner.state).protocol_version.clone()
    }

    fn set_protocol_version(&self, version: Option<&str>) {
        lock(&self.inner.state).protocol_version = version.map(str::to_owned);
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            supports_protocol_version_discovery: true,
            supports_tool_parameter_headers: true,
        }
    }
}
