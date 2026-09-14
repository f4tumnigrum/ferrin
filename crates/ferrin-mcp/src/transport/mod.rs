//! Transports carrying JSON-RPC messages to and from an MCP server.

use std::sync::Arc;

use ferrin_spec::Headers;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use tokio_util::sync::CancellationToken;

use crate::error::McpError;
use crate::protocol::JsonRpcMessage;

mod common;
mod headers;
mod http;
mod http_config;
mod sse;
#[cfg(feature = "stdio")]
mod stdio;

pub use headers::HeaderBinding;
pub use headers::HeaderValueType;
pub use headers::encode_header_value;
pub use headers::header_bindings;
pub use headers::tool_headers;
pub use http::HttpTransport;
pub use http_config::HttpTransportConfig;
pub use http_config::ReconnectionOptions;
pub use http_config::RedirectMode;
pub use http_config::SessionHook;
pub use sse::SseTransport;
pub use sse::SseTransportConfig;
#[cfg(feature = "stdio")]
pub use stdio::DEFAULT_INHERITED_ENV_VARS;
#[cfg(feature = "stdio")]
pub use stdio::StdioConfig;
#[cfg(feature = "stdio")]
pub use stdio::StdioStderr;
#[cfg(feature = "stdio")]
pub use stdio::StdioTransport;

/// Per-message options of [`McpTransport::send`].
#[derive(Debug, Clone, Default)]
pub struct SendOptions {
    /// Cancels this message (and the request it belongs to).
    pub cancellation: Option<CancellationToken>,
    /// Request-specific HTTP headers derived from tool parameters
    /// (`Mcp-Param-*`); only HTTP transports use them.
    pub headers: Headers,
}

/// Options of [`McpTransport::close`].
#[derive(Debug, Clone, Default)]
pub struct CloseOptions {
    /// Cancels the cleanup (session termination).
    pub cancellation: Option<CancellationToken>,
}

/// What a transport emits on its incoming stream.
#[derive(Debug)]
#[non_exhaustive]
pub enum TransportEvent {
    /// A message from the server.
    Message(JsonRpcMessage),
    /// A failure that did not close the transport (parse error, failed
    /// inbound stream).
    Error(McpError),
    /// The transport is closed; no further events follow.
    Closed,
}

/// Static properties of a transport.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransportCapabilities {
    /// Whether the client may probe `server/discover` before falling back to
    /// `initialize`.
    pub supports_protocol_version_discovery: bool,
    /// Whether `x-mcp-header` tool parameters are mirrored into request
    /// headers.
    pub supports_tool_parameter_headers: bool,
}

/// A bidirectional JSON-RPC channel.
///
/// Implementations emit server messages, errors and the final
/// [`TransportEvent::Closed`] on the stream returned by
/// [`McpTransport::incoming`], which is consumed by exactly one reader.
pub trait McpTransport: Send + Sync + 'static {
    /// Establishes the connection.
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>>;

    /// Sends one message.
    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>>;

    /// Takes the incoming event stream. Later calls return an empty stream.
    fn incoming(&self) -> BoxStream<'static, TransportEvent>;

    /// Closes the connection.
    fn close(&self, options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>>;

    /// The protocol version currently in use.
    fn protocol_version(&self) -> Option<String>;

    /// Sets the protocol version negotiated by the client; `None` clears it
    /// (before a legacy `initialize` after a failed discovery).
    fn set_protocol_version(&self, version: Option<&str>);

    /// Static properties.
    fn capabilities(&self) -> TransportCapabilities;
}

/// Shared transport handle.
pub type SharedMcpTransport = Arc<dyn McpTransport>;

/// How the client reaches the server.
#[derive(Clone)]
#[non_exhaustive]
pub enum TransportConfig {
    /// Streamable HTTP (2025-11-25 and 2026-07-28).
    Http(HttpTransportConfig),
    /// Legacy HTTP with SSE (2024-11-05).
    Sse(SseTransportConfig),
    /// Child process over stdio.
    #[cfg(feature = "stdio")]
    Stdio(StdioConfig),
    /// A caller-provided transport.
    Custom(SharedMcpTransport),
}

impl std::fmt::Debug for TransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(config) => f.debug_tuple("Http").field(config).finish(),
            Self::Sse(config) => f.debug_tuple("Sse").field(config).finish(),
            #[cfg(feature = "stdio")]
            Self::Stdio(config) => f.debug_tuple("Stdio").field(config).finish(),
            Self::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

impl TransportConfig {
    /// Streamable HTTP transport for `url` with default settings.
    #[must_use]
    pub fn http(url: url::Url) -> Self {
        Self::Http(HttpTransportConfig::new(url))
    }

    /// Legacy SSE transport for `url` with default settings.
    #[must_use]
    pub fn sse(url: url::Url) -> Self {
        Self::Sse(SseTransportConfig::new(url))
    }

    /// Stdio transport running `command`.
    #[cfg(feature = "stdio")]
    #[must_use]
    pub fn stdio(command: impl Into<String>) -> Self {
        Self::Stdio(StdioConfig::new(command))
    }

    /// Builds the transport.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Transport`] when the default HTTP client cannot be
    /// built.
    pub fn build(self) -> Result<SharedMcpTransport, McpError> {
        Ok(match self {
            Self::Http(config) => Arc::new(HttpTransport::new(config)?),
            Self::Sse(config) => Arc::new(SseTransport::new(config)?),
            #[cfg(feature = "stdio")]
            Self::Stdio(config) => Arc::new(StdioTransport::new(config)),
            Self::Custom(transport) => transport,
            #[allow(unreachable_patterns, reason = "TransportConfig is non-exhaustive")]
            _ => {
                return Err(McpError::invalid_argument(
                    "unsupported transport configuration",
                ));
            }
        })
    }
}

/// Runs `future` until `cancellation` fires.
pub(crate) async fn with_cancellation<T>(
    cancellation: Option<&CancellationToken>,
    future: impl Future<Output = Result<T, McpError>>,
) -> Result<T, McpError> {
    match cancellation {
        Some(token) => tokio::select! {
            () = token.cancelled() => Err(McpError::Cancelled),
            result = future => result,
        },
        None => future.await,
    }
}

/// Receiver side of the event channel, handed out once by `incoming()`.
pub(crate) struct EventChannel {
    sender: tokio::sync::mpsc::UnboundedSender<TransportEvent>,
    receiver: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<TransportEvent>>>,
}

impl EventChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        Self {
            sender,
            receiver: std::sync::Mutex::new(Some(receiver)),
        }
    }

    pub(crate) fn emit(&self, event: TransportEvent) {
        // A dropped receiver means nobody listens any more; dropping the
        // event is the intended behaviour.
        let _ = self.sender.send(event);
    }

    pub(crate) fn take(&self) -> BoxStream<'static, TransportEvent> {
        let receiver = self
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match receiver {
            Some(receiver) => Box::pin(futures_util::stream::unfold(
                receiver,
                |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
            )),
            None => Box::pin(futures_util::stream::empty()),
        }
    }
}

impl std::fmt::Debug for EventChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventChannel")
    }
}

/// Locks a mutex, recovering from poisoning.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
