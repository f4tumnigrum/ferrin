//! Shared helpers: an in-process mock transport and JSON builders.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpClientConfig;
use ferrin_mcp::McpError;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::protocol::JsonRpcRequest;
use ferrin_mcp::protocol::LATEST_LEGACY_PROTOCOL_VERSION;
use ferrin_mcp::protocol::LATEST_PROTOCOL_VERSION;
use ferrin_mcp::transport::CloseOptions;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::TransportCapabilities;
use ferrin_mcp::transport::TransportConfig;
use ferrin_mcp::transport::TransportEvent;
use ferrin_provider_util::secure_url::UrlPolicy;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

/// URL policy accepting the loopback fixture server.
pub(crate) fn local_policy() -> UrlPolicy {
    UrlPolicy::new().allow_http().allow_private_networks()
}

/// Answers one outgoing message with zero or more incoming messages.
pub(crate) type Responder =
    Arc<dyn Fn(&JsonRpcMessage) -> Result<Vec<JsonRpcMessage>, McpError> + Send + Sync>;

/// A transport whose server side is a closure.
pub(crate) struct MockTransport {
    responder: Responder,
    sender: mpsc::UnboundedSender<TransportEvent>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<TransportEvent>>>,
    sent: Mutex<Vec<JsonRpcMessage>>,
    sent_headers: Mutex<Vec<ferrin_spec::Headers>>,
    protocol_version: Mutex<Option<String>>,
    capabilities: TransportCapabilities,
    closed: AtomicBool,
}

impl MockTransport {
    pub(crate) fn new(
        capabilities: TransportCapabilities,
        responder: impl Fn(&JsonRpcMessage) -> Result<Vec<JsonRpcMessage>, McpError>
        + Send
        + Sync
        + 'static,
    ) -> Arc<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Arc::new(Self {
            responder: Arc::new(responder),
            sender,
            receiver: Mutex::new(Some(receiver)),
            sent: Mutex::new(Vec::new()),
            sent_headers: Mutex::new(Vec::new()),
            protocol_version: Mutex::new(None),
            capabilities,
            closed: AtomicBool::new(false),
        })
    }

    /// Delivers a server-initiated event.
    pub(crate) fn inject(&self, event: TransportEvent) {
        self.sender.send(event).unwrap();
    }

    /// Everything the client sent.
    pub(crate) fn sent(&self) -> Vec<JsonRpcMessage> {
        self.sent.lock().unwrap().clone()
    }

    pub(crate) fn sent_headers(&self) -> Vec<ferrin_spec::Headers> {
        self.sent_headers.lock().unwrap().clone()
    }

    /// Requests the client sent for `method`.
    pub(crate) fn requests(&self, method: &str) -> Vec<JsonRpcRequest> {
        self.sent()
            .into_iter()
            .filter_map(|message| match message {
                JsonRpcMessage::Request(request) if request.method == method => Some(request),
                _ => None,
            })
            .collect()
    }

    /// Methods of every sent message, in order.
    pub(crate) fn methods(&self) -> Vec<String> {
        self.sent()
            .iter()
            .filter_map(|message| message.method().map(str::to_owned))
            .collect()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl McpTransport for MockTransport {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async { Ok(()) })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            self.sent.lock().unwrap().push(message.clone());
            self.sent_headers.lock().unwrap().push(options.headers);
            for reply in (self.responder)(&message)? {
                self.sender.send(TransportEvent::Message(reply)).unwrap();
            }
            Ok(())
        })
    }

    fn incoming(&self) -> BoxStream<'static, TransportEvent> {
        let receiver = self.receiver.lock().unwrap().take().unwrap();
        Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
        ))
    }

    fn close(&self, _options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            self.closed.store(true, Ordering::SeqCst);
            let _ = self.sender.send(TransportEvent::Closed);
            Ok(())
        })
    }

    fn protocol_version(&self) -> Option<String> {
        self.protocol_version.lock().unwrap().clone()
    }

    fn set_protocol_version(&self, version: Option<&str>) {
        *self.protocol_version.lock().unwrap() = version.map(str::to_owned);
    }

    fn capabilities(&self) -> TransportCapabilities {
        self.capabilities
    }
}

pub(crate) fn discovery_capabilities() -> TransportCapabilities {
    TransportCapabilities {
        supports_protocol_version_discovery: true,
        supports_tool_parameter_headers: true,
    }
}

/// A success response to `request` with `result`.
pub(crate) fn reply(request: &JsonRpcMessage, result: Value) -> JsonRpcMessage {
    JsonRpcMessage::response(
        request.id().cloned().unwrap(),
        result.as_object().cloned().unwrap(),
    )
}

/// An error response to `request`.
pub(crate) fn reply_error(request: &JsonRpcMessage, code: i64, message: &str) -> JsonRpcMessage {
    JsonRpcMessage::error(request.id().cloned(), code, message, None)
}

/// Adds `resultType: complete` to a modern result.
pub(crate) fn complete(mut result: Value) -> Value {
    if result.get("resultType").is_none() {
        result["resultType"] = json!("complete");
    }
    result
}

pub(crate) fn server_capabilities() -> Value {
    json!({
        "tools": {"listChanged": true},
        "resources": {"subscribe": false},
        "prompts": {},
        "completions": {}
    })
}

pub(crate) fn discover_result() -> Value {
    json!({
        "resultType": "complete",
        "supportedVersions": [LATEST_PROTOCOL_VERSION, LATEST_LEGACY_PROTOCOL_VERSION],
        "capabilities": server_capabilities(),
        "instructions": "Use the tools wisely.",
        "_meta": {
            "io.modelcontextprotocol/serverInfo": {"name": "mock-server", "version": "1.2.3"}
        }
    })
}

pub(crate) fn initialize_result(version: &str) -> Value {
    json!({
        "protocolVersion": version,
        "capabilities": server_capabilities(),
        "serverInfo": {"name": "legacy-server", "version": "0.9.0"},
        "instructions": "Legacy instructions."
    })
}

pub(crate) fn tool_definition(name: &str) -> Value {
    json!({
        "name": name,
        "title": format!("{name} title"),
        "description": format!("{name} description"),
        "inputSchema": {
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        },
        "annotations": {"readOnlyHint": true}
    })
}

pub(crate) fn call_tool_result(text: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": false
    })
}

/// Handler answering non-negotiation requests: `Ok(result)` or
/// `Err((code, message))`.
pub(crate) type Handler =
    Arc<dyn Fn(&JsonRpcRequest) -> Result<Value, (i64, String)> + Send + Sync>;

fn answer(
    message: &JsonRpcMessage,
    request: &JsonRpcRequest,
    handler: &Handler,
) -> Vec<JsonRpcMessage> {
    match handler(request) {
        Ok(result) => vec![reply(message, result)],
        Err((code, text)) => vec![reply_error(message, code, &text)],
    }
}

/// A transport speaking the modern protocol: `server/discover` succeeds and
/// every other request goes to `handler` (results get `resultType`).
pub(crate) fn modern_transport(
    handler: impl Fn(&JsonRpcRequest) -> Result<Value, (i64, String)> + Send + Sync + 'static,
) -> Arc<MockTransport> {
    let handler: Handler = Arc::new(handler);
    MockTransport::new(discovery_capabilities(), move |message| {
        let JsonRpcMessage::Request(request) = message else {
            return Ok(Vec::new());
        };
        if request.method == "server/discover" {
            return Ok(vec![reply(message, discover_result())]);
        }
        Ok(match handler(request) {
            Ok(result) => vec![reply(message, complete(result))],
            Err((code, text)) => vec![reply_error(message, code, &text)],
        })
    })
}

/// A transport speaking the legacy protocol: `server/discover` is unknown,
/// `initialize` succeeds with `version`.
pub(crate) fn legacy_transport(
    version: &'static str,
    handler: impl Fn(&JsonRpcRequest) -> Result<Value, (i64, String)> + Send + Sync + 'static,
) -> Arc<MockTransport> {
    let handler: Handler = Arc::new(handler);
    MockTransport::new(discovery_capabilities(), move |message| {
        let JsonRpcMessage::Request(request) = message else {
            return Ok(Vec::new());
        };
        Ok(match request.method.as_str() {
            "server/discover" => vec![reply_error(message, -32601, "method not found")],
            "initialize" => vec![reply(message, initialize_result(version))],
            _ => answer(message, request, &handler),
        })
    })
}

pub(crate) fn config(transport: Arc<MockTransport>) -> McpClientConfig {
    McpClientConfig::new(TransportConfig::Custom(transport))
}

pub(crate) async fn connect(transport: Arc<MockTransport>) -> McpClient {
    McpClient::connect(config(transport)).await.unwrap()
}

/// Waits until `predicate` holds for the messages sent so far.
pub(crate) async fn wait_for_sent(
    transport: &MockTransport,
    predicate: impl Fn(&[JsonRpcMessage]) -> bool,
) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if predicate(&transport.sent()) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expected message was not sent");
}
