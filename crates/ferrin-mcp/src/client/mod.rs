//! The MCP client: connection lifecycle, message dispatch and configuration.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::McpError;
use crate::protocol::ClientCapabilities;
use crate::protocol::ElicitResult;
use crate::protocol::ElicitationRequest;
use crate::protocol::Implementation;
use crate::protocol::JsonRpcMessage;
use crate::protocol::JsonRpcNotification;
use crate::protocol::ProtocolEra;
use crate::protocol::RequestId;
use crate::protocol::ServerCapabilities;
use crate::transport::CloseOptions;
use crate::transport::SharedMcpTransport;
use crate::transport::TransportConfig;
use crate::transport::TransportEvent;
use crate::transport::lock;

mod methods;
mod request;
mod server_requests;

pub use request::RequestOptions;

/// Default client name sent in `clientInfo`.
pub const DEFAULT_CLIENT_NAME: &str = "ferrin-mcp-client";

/// Default `server/discover` timeout.
pub const DEFAULT_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Default upper bound of `input_required` rounds per request.
pub const DEFAULT_MAX_INPUT_ROUNDS: u32 = 8;

/// Receives errors that have no request to fail (parse errors, failed inbound
/// streams, unmatched responses, failed server-request handling).
pub type UncaughtErrorHook = Arc<dyn Fn(McpError) + Send + Sync>;

/// Receives server notifications.
pub type NotificationHook = Arc<dyn Fn(&JsonRpcNotification) + Send + Sync>;

/// Answers `elicitation/create` requests from the server.
pub trait ElicitationHandler: Send + Sync {
    /// Produces the user's answer to `request`.
    fn handle(&self, request: ElicitationRequest) -> BoxFuture<'_, Result<ElicitResult, McpError>>;
}

struct ElicitationFn<F>(F);

impl<F, Fut> ElicitationHandler for ElicitationFn<F>
where
    F: Fn(ElicitationRequest) -> Fut + Send + Sync,
    Fut: Future<Output = Result<ElicitResult, McpError>> + Send + 'static,
{
    fn handle(&self, request: ElicitationRequest) -> BoxFuture<'_, Result<ElicitResult, McpError>> {
        Box::pin((self.0)(request))
    }
}

/// Wraps an async closure as an [`ElicitationHandler`].
pub fn elicitation_handler<F, Fut>(function: F) -> Arc<dyn ElicitationHandler>
where
    F: Fn(ElicitationRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<ElicitResult, McpError>> + Send + 'static,
{
    Arc::new(ElicitationFn(function))
}

/// Configuration of [`McpClient::connect`].
#[derive(Clone)]
#[non_exhaustive]
pub struct McpClientConfig {
    /// Transport to the server.
    pub transport: TransportConfig,
    /// Client name (`clientInfo.name`).
    pub name: String,
    /// Client version (`clientInfo.version`).
    pub version: String,
    /// Client title (`clientInfo.title`).
    pub title: Option<String>,
    /// Capabilities announced to the server.
    pub capabilities: ClientCapabilities,
    /// Retries of a failed `tools/call` on retryable transport errors
    /// (default 0).
    pub max_tool_call_retries: u32,
    /// Timeout applied to requests without an explicit one.
    pub default_request_timeout: Option<Duration>,
    /// Whether `server/discover` is tried before `initialize` (default `true`).
    pub protocol_discovery: bool,
    /// Timeout of the `server/discover` probe (default 1 s).
    pub discovery_timeout: Duration,
    /// Timeout of the legacy `initialize` request.
    pub initialization_timeout: Option<Duration>,
    /// Maximum `input_required` rounds per request (default 8).
    pub max_input_rounds: u32,
    /// Called for errors that cannot be attributed to a request.
    pub on_uncaught_error: Option<UncaughtErrorHook>,
    /// Called for every server notification.
    pub on_notification: Option<NotificationHook>,
    /// Handler of `elicitation/create` requests.
    pub elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
}

impl std::fmt::Debug for McpClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClientConfig")
            .field("transport", &self.transport)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("title", &self.title)
            .field("capabilities", &self.capabilities)
            .field("max_tool_call_retries", &self.max_tool_call_retries)
            .field("default_request_timeout", &self.default_request_timeout)
            .field("protocol_discovery", &self.protocol_discovery)
            .field("discovery_timeout", &self.discovery_timeout)
            .field("initialization_timeout", &self.initialization_timeout)
            .field("max_input_rounds", &self.max_input_rounds)
            .field("elicitation_handler", &self.elicitation_handler.is_some())
            .finish_non_exhaustive()
    }
}

impl McpClientConfig {
    /// Default configuration for `transport`.
    #[must_use]
    pub fn new(transport: TransportConfig) -> Self {
        Self {
            transport,
            name: DEFAULT_CLIENT_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            title: None,
            capabilities: ClientCapabilities::default(),
            max_tool_call_retries: 0,
            default_request_timeout: None,
            protocol_discovery: true,
            discovery_timeout: DEFAULT_DISCOVERY_TIMEOUT,
            initialization_timeout: None,
            max_input_rounds: DEFAULT_MAX_INPUT_ROUNDS,
            on_uncaught_error: None,
            on_notification: None,
            elicitation_handler: None,
        }
    }

    /// Sets the client name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the client version.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Sets the client title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the announced capabilities.
    #[must_use]
    pub fn capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Sets the `tools/call` retry count.
    #[must_use]
    pub fn max_tool_call_retries(mut self, retries: u32) -> Self {
        self.max_tool_call_retries = retries;
        self
    }

    /// Sets the default request timeout.
    #[must_use]
    pub fn default_request_timeout(mut self, timeout: Duration) -> Self {
        self.default_request_timeout = Some(timeout);
        self
    }

    /// Enables or disables the `server/discover` probe.
    #[must_use]
    pub fn protocol_discovery(mut self, enabled: bool) -> Self {
        self.protocol_discovery = enabled;
        self
    }

    /// Sets the discovery timeout.
    #[must_use]
    pub fn discovery_timeout(mut self, timeout: Duration) -> Self {
        self.discovery_timeout = timeout;
        self
    }

    /// Sets the `initialize` timeout.
    #[must_use]
    pub fn initialization_timeout(mut self, timeout: Duration) -> Self {
        self.initialization_timeout = Some(timeout);
        self
    }

    /// Sets the uncaught-error hook.
    #[must_use]
    pub fn on_uncaught_error(mut self, hook: UncaughtErrorHook) -> Self {
        self.on_uncaught_error = Some(hook);
        self
    }

    /// Sets the notification hook.
    #[must_use]
    pub fn on_notification(mut self, hook: NotificationHook) -> Self {
        self.on_notification = Some(hook);
        self
    }

    /// Sets the elicitation handler.
    #[must_use]
    pub fn elicitation_handler(mut self, handler: Arc<dyn ElicitationHandler>) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    fn client_info(&self) -> Implementation {
        Implementation {
            name: self.name.clone(),
            version: self.version.clone(),
            title: self.title.clone(),
        }
    }
}

/// Negotiated server state.
#[derive(Debug, Default)]
pub(crate) struct ServerState {
    pub(crate) capabilities: Option<ServerCapabilities>,
    pub(crate) info: Option<Implementation>,
    pub(crate) instructions: Option<String>,
    pub(crate) protocol_version: Option<String>,
    pub(crate) closed: bool,
}

impl ServerState {
    fn era(&self) -> ProtocolEra {
        self.protocol_version
            .as_deref()
            .map_or(ProtocolEra::Legacy, ProtocolEra::of_version)
    }
}

type Pending = HashMap<i64, oneshot::Sender<Result<ferrin_spec::JsonObject, McpError>>>;

pub(crate) struct ClientInner {
    pub(crate) transport: SharedMcpTransport,
    pub(crate) config: McpClientConfig,
    pub(crate) state: Mutex<ServerState>,
    pub(crate) pending: Mutex<Pending>,
    pub(crate) elicitation: Mutex<Option<Arc<dyn ElicitationHandler>>>,
    next_id: AtomicI64,
    cancellation: CancellationToken,
}

impl ClientInner {
    pub(crate) fn era(&self) -> ProtocolEra {
        lock(&self.state).era()
    }

    pub(crate) fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    pub(crate) fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn client_info(&self) -> Implementation {
        self.config.client_info()
    }

    pub(crate) fn report(&self, error: McpError) {
        match &self.config.on_uncaught_error {
            Some(hook) => hook(error),
            None => tracing::warn!(error = %error, "uncaught MCP error"),
        }
    }

    pub(crate) fn register(
        &self,
        id: i64,
    ) -> oneshot::Receiver<Result<ferrin_spec::JsonObject, McpError>> {
        let (sender, receiver) = oneshot::channel();
        lock(&self.pending).insert(id, sender);
        receiver
    }

    pub(crate) fn unregister(&self, id: i64) {
        lock(&self.pending).remove(&id);
    }

    fn fail_pending(&self, error: impl Fn() -> McpError) {
        let pending = std::mem::take(&mut *lock(&self.pending));
        for (_, sender) in pending {
            let _ = sender.send(Err(error()));
        }
    }

    fn resolve(&self, id: &RequestId, outcome: Result<ferrin_spec::JsonObject, McpError>) -> bool {
        let RequestId::Number(number) = id else {
            return false;
        };
        let sender = lock(&self.pending).remove(number);
        match sender {
            Some(sender) => {
                let _ = sender.send(outcome);
                true
            }
            None => false,
        }
    }

    fn handle_message(self: &Arc<Self>, message: JsonRpcMessage, handlers: &mut JoinSet<()>) {
        match message {
            JsonRpcMessage::Response(response) => {
                if !self.resolve(&response.id, Ok(response.result)) {
                    self.report(McpError::protocol(format!(
                        "received a response for unknown request id {}",
                        response.id
                    )));
                }
            }
            JsonRpcMessage::Error(error) => {
                let outcome = McpError::JsonRpc {
                    code: error.error.code,
                    message: error.error.message,
                    data: error.error.data,
                };
                match &error.id {
                    Some(id) if self.resolve(id, Err(outcome_clone(&outcome))) => {}
                    _ => self.report(outcome),
                }
            }
            JsonRpcMessage::Notification(notification) => {
                if let Some(hook) = &self.config.on_notification {
                    hook(&notification);
                } else {
                    tracing::debug!(method = %notification.method, "MCP notification");
                }
            }
            JsonRpcMessage::Request(request) => {
                let inner = Arc::clone(self);
                handlers.spawn(async move { inner.handle_server_request(request).await });
            }
            #[allow(unreachable_patterns, reason = "JsonRpcMessage is non-exhaustive")]
            _ => {}
        }
    }

    async fn dispatch(self: Arc<Self>, mut incoming: BoxStream<'static, TransportEvent>) {
        let mut handlers = JoinSet::new();
        loop {
            let event = tokio::select! {
                () = self.cancellation.cancelled() => break,
                event = incoming.next() => match event {
                    Some(event) => event,
                    None => break,
                },
                Some(_) = handlers.join_next(), if !handlers.is_empty() => continue,
            };
            match event {
                TransportEvent::Message(message) => self.handle_message(message, &mut handlers),
                TransportEvent::Error(error) => self.report(error),
                TransportEvent::Closed => break,
                #[allow(unreachable_patterns, reason = "TransportEvent is non-exhaustive")]
                _ => {}
            }
        }
        lock(&self.state).closed = true;
        self.fail_pending(|| McpError::Closed);
        handlers.abort_all();
    }
}

/// Clones a JSON-RPC error for delivery to both the request and the hook.
fn outcome_clone(error: &McpError) -> McpError {
    match error {
        McpError::JsonRpc {
            code,
            message,
            data,
        } => McpError::JsonRpc {
            code: *code,
            message: message.clone(),
            data: data.clone(),
        },
        other => McpError::protocol(other.to_string()),
    }
}

/// A connected MCP client.
///
/// # Examples
///
/// ```no_run
/// use ferrin_mcp::McpClient;
/// use ferrin_mcp::McpClientConfig;
/// use ferrin_mcp::transport::TransportConfig;
/// use url::Url;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let url = Url::parse("https://mcp.example.com/mcp")?;
/// let client = McpClient::connect(McpClientConfig::new(TransportConfig::http(url))).await?;
/// let tools = client.tools(Default::default()).await?;
/// println!("{} tools", tools.len());
/// client.close().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct McpClient {
    pub(crate) inner: Arc<ClientInner>,
    // Ownership stays with public handles; dispatched futures only own `inner`.
    tasks: Arc<Mutex<JoinSet<()>>>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("state", &lock(&self.inner.state))
            .finish_non_exhaustive()
    }
}

impl McpClient {
    /// Connects to the server and negotiates the protocol version.
    ///
    /// # Errors
    ///
    /// Returns the transport or negotiation failure; the transport is closed
    /// before the error is returned.
    #[tracing::instrument(skip_all, fields(client = %config.name))]
    pub async fn connect(config: McpClientConfig) -> Result<Self, McpError> {
        let transport = config.transport.clone().build()?;
        if let Err(error) = transport.start().await {
            let _ = transport.close(CloseOptions::default()).await;
            return Err(error);
        }
        let inner = Arc::new(ClientInner {
            transport,
            elicitation: Mutex::new(config.elicitation_handler.clone()),
            config,
            state: Mutex::new(ServerState::default()),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            cancellation: CancellationToken::new(),
        });
        let incoming = inner.transport.incoming();
        let mut tasks = JoinSet::new();
        tasks.spawn(Arc::clone(&inner).dispatch(incoming));
        let client = Self {
            inner,
            tasks: Arc::new(Mutex::new(tasks)),
        };
        if let Err(error) = client.initialize().await {
            let _ = client.close().await;
            return Err(error);
        }
        Ok(client)
    }

    /// Replaces the elicitation handler.
    pub fn on_elicitation(&self, handler: Arc<dyn ElicitationHandler>) {
        *lock(&self.inner.elicitation) = Some(handler);
    }

    /// Capabilities declared by the server.
    #[must_use]
    pub fn server_capabilities(&self) -> Option<ServerCapabilities> {
        lock(&self.inner.state).capabilities.clone()
    }

    /// Server implementation info.
    #[must_use]
    pub fn server_info(&self) -> Option<Implementation> {
        lock(&self.inner.state).info.clone()
    }

    /// Instructions the server provided for the model.
    #[must_use]
    pub fn instructions(&self) -> Option<String> {
        lock(&self.inner.state).instructions.clone()
    }

    /// Negotiated protocol version.
    #[must_use]
    pub fn protocol_version(&self) -> Option<String> {
        lock(&self.inner.state).protocol_version.clone()
    }

    /// Era of the negotiated protocol version.
    #[must_use]
    pub fn protocol_era(&self) -> ProtocolEra {
        self.inner.era()
    }

    /// Whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Closes the transport and fails pending requests.
    ///
    /// # Errors
    ///
    /// Returns the transport's close failure (for example a failed legacy
    /// session termination); the client is closed regardless.
    pub async fn close(&self) -> Result<(), McpError> {
        let inner = &self.inner;
        {
            let mut state = lock(&inner.state);
            if state.closed {
                return Ok(());
            }
            state.closed = true;
        }
        inner.cancellation.cancel();
        let result = inner.transport.close(CloseOptions::default()).await;
        inner.fail_pending(|| McpError::Closed);
        lock(&self.tasks).abort_all();
        result
    }
}
