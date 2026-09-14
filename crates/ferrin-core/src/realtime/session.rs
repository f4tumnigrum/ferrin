//! Connection task of a realtime session.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use ferrin_spec::DynRealtimeModel;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::realtime_model::ClientSecret;
use ferrin_spec::realtime_model::ConversationItem;
use ferrin_spec::realtime_model::ConversationRole;
use ferrin_spec::realtime_model::RealtimeClientEvent;
use ferrin_spec::realtime_model::RealtimeServerEvent;
use ferrin_spec::realtime_model::RealtimeSessionConfig;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolSet;
use ferrin_tool::execute_to_completion;
use futures_util::SinkExt;
use futures_util::StreamExt;
use http::HeaderValue;
use http::header::SEC_WEBSOCKET_PROTOCOL;
use http::header::USER_AGENT;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;

use super::RealtimeEvent;
use super::tools::ToolTurn;
use crate::error::Error;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Capacity of the outbound message queue.
const OUTBOUND_BUFFER: usize = 64;

/// State shared between the connection task and the handles.
struct Shared {
    model: Arc<dyn DynRealtimeModel>,
    outbound: mpsc::Sender<JsonValue>,
    cancellation: CancellationToken,
    closed: AtomicBool,
    tool_turn: Mutex<ToolTurn>,
}

impl Shared {
    fn tool_turn(&self) -> std::sync::MutexGuard<'_, ToolTurn> {
        self.tool_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Sends events into a realtime session and closes it.
///
/// Handles are cheap to clone and remain valid until the session ends.
#[derive(Clone)]
pub struct RealtimeHandle {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for RealtimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealtimeHandle")
            .field("provider", self.shared.model.provider())
            .field("model_id", self.shared.model.model_id())
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl RealtimeHandle {
    /// Sends a client event.
    ///
    /// # Errors
    ///
    /// Fails when the model cannot serialize the event or the session is
    /// closed.
    pub async fn send(&self, event: RealtimeClientEvent) -> Result<(), Error> {
        let raw = self.shared.model.serialize_client_event(event).await?;
        self.send_raw(raw).await
    }

    /// Sends a raw provider message.
    ///
    /// # Errors
    ///
    /// Fails when the session is closed.
    pub async fn send_raw(&self, raw: JsonValue) -> Result<(), Error> {
        self.shared
            .outbound
            .send(raw)
            .await
            .map_err(|_| closed_error())
    }

    /// Sends a user text message and requests a response.
    ///
    /// # Errors
    ///
    /// See [`RealtimeHandle::send`].
    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), Error> {
        self.send(RealtimeClientEvent::ConversationItemCreate {
            item: ConversationItem::TextMessage {
                role: ConversationRole::User,
                text: text.into(),
            },
        })
        .await?;
        self.send(RealtimeClientEvent::ResponseCreate { options: None })
            .await
    }

    /// Submits the output of a tool call. A follow-up response is requested
    /// once every tool call of the response has an output.
    ///
    /// # Errors
    ///
    /// See [`RealtimeHandle::send`].
    pub async fn add_tool_output(&self, call_id: &str, output: &JsonValue) -> Result<(), Error> {
        let name = self
            .shared
            .tool_turn()
            .name(call_id)
            .map(|name| name.as_str().to_owned());
        self.send(RealtimeClientEvent::ConversationItemCreate {
            item: ConversationItem::FunctionCallOutput {
                call_id: call_id.to_owned(),
                name,
                output: output.to_string(),
            },
        })
        .await?;
        let request_response = self.shared.tool_turn().output_submitted(call_id);
        if request_response {
            self.send(RealtimeClientEvent::ResponseCreate { options: None })
                .await?;
        }
        Ok(())
    }

    /// Requests a response with the default options.
    ///
    /// # Errors
    ///
    /// See [`RealtimeHandle::send`].
    pub async fn request_response(&self) -> Result<(), Error> {
        self.send(RealtimeClientEvent::ResponseCreate { options: None })
            .await
    }

    /// Asks the connection task to close the connection.
    pub fn close(&self) {
        self.shared.cancellation.cancel();
    }

    /// Returns `true` once the connection task has finished.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
}

fn closed_error() -> Error {
    Error::message("realtime session is closed")
}

/// Inputs of [`start`].
pub(super) struct StartOptions {
    pub(super) model: Arc<dyn DynRealtimeModel>,
    pub(super) secret: ClientSecret,
    pub(super) config: RealtimeSessionConfig,
    pub(super) tools: Arc<ToolSet>,
    pub(super) tools_context: Option<JsonValue>,
    pub(super) cancellation: CancellationToken,
    pub(super) events: mpsc::Sender<RealtimeEvent>,
}

/// Opens the WebSocket, sends the initial `session-update` and spawns the
/// connection task into `tasks`.
pub(super) async fn start(
    options: StartOptions,
    tasks: &mut JoinSet<()>,
) -> Result<RealtimeHandle, Error> {
    let StartOptions {
        model,
        secret,
        config,
        tools,
        tools_context,
        cancellation,
        events,
    } = options;
    let ws_config = model.websocket_config(&secret.token, &secret.url);
    let mut request = ws_config
        .url
        .as_str()
        .into_client_request()
        .map_err(Error::other)?;
    if !ws_config.protocols.is_empty() {
        let value = HeaderValue::from_str(&ws_config.protocols.join(", "))
            .map_err(|error| Error::invalid_argument("protocols", error.to_string()))?;
        request.headers_mut().insert(SEC_WEBSOCKET_PROTOCOL, value);
    }
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static(crate::USER_AGENT));

    let connect = connect_async_tls_with_config(request, None, false, None);
    let (mut socket, _response) = tokio::select! {
        result = connect => result.map_err(Error::other)?,
        () = cancellation.cancelled() => return Err(Error::Cancelled),
    };

    let initial = model
        .serialize_client_event(RealtimeClientEvent::SessionUpdate {
            config: Box::new(config),
        })
        .await?;
    socket
        .send(Message::text(initial.to_string()))
        .await
        .map_err(Error::other)?;

    let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_BUFFER);
    let shared = Arc::new(Shared {
        model,
        outbound: outbound_tx,
        cancellation,
        closed: AtomicBool::new(false),
        tool_turn: Mutex::new(ToolTurn::default()),
    });
    let handle = RealtimeHandle {
        shared: Arc::clone(&shared),
    };
    tasks.spawn(run(Connection {
        socket,
        outbound: outbound_rx,
        shared,
        events,
        tools,
        tools_context,
    }));
    Ok(handle)
}

struct Connection {
    socket: Socket,
    outbound: mpsc::Receiver<JsonValue>,
    shared: Arc<Shared>,
    events: mpsc::Sender<RealtimeEvent>,
    tools: Arc<ToolSet>,
    tools_context: Option<JsonValue>,
}

async fn run(mut connection: Connection) {
    let mut tool_tasks: JoinSet<()> = JoinSet::new();
    loop {
        let stop = tokio::select! {
            biased;
            () = connection.shared.cancellation.cancelled() => {
                let _ = connection.socket.send(Message::Close(None)).await;
                true
            }
            outbound = connection.outbound.recv() => match outbound {
                Some(raw) => connection.send_raw(raw).await.is_err(),
                None => {
                    let _ = connection.socket.send(Message::Close(None)).await;
                    true
                }
            },
            frame = connection.socket.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    connection.handle_text(text.as_str(), &mut tool_tasks).await
                }
                Some(Ok(Message::Binary(bytes))) => match std::str::from_utf8(&bytes) {
                    Ok(text) => connection.handle_text(text, &mut tool_tasks).await,
                    Err(_) => false,
                },
                Some(Ok(Message::Close(_))) | None => true,
                Some(Ok(_)) => false,
                Some(Err(error)) => {
                    connection.emit(Err(Error::other(error))).await;
                    true
                }
            },
            Some(joined) = tool_tasks.join_next(), if !tool_tasks.is_empty() => {
                if let Err(error) = joined
                    && !error.is_cancelled()
                {
                    connection.emit(Err(Error::other(error))).await;
                }
                false
            }
        };
        if stop {
            break;
        }
    }
    tool_tasks.shutdown().await;
    connection.shared.closed.store(true, Ordering::Release);
    connection.shared.cancellation.cancel();
}

impl Connection {
    /// Forwards an item to the consumer; returns `false` when the consumer is
    /// gone.
    async fn emit(&self, item: RealtimeEvent) -> bool {
        self.events.send(item).await.is_ok()
    }

    async fn send_raw(&mut self, raw: JsonValue) -> Result<(), ()> {
        match self.socket.send(Message::text(raw.to_string())).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.emit(Err(Error::other(error))).await;
                Err(())
            }
        }
    }

    /// Handles one text frame; returns `true` when the loop must stop.
    async fn handle_text(&mut self, text: &str, tool_tasks: &mut JoinSet<()>) -> bool {
        let raw: JsonValue = match serde_json::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                tracing::debug!(error = %error, "ignoring non-JSON realtime message");
                return false;
            }
        };
        if let Some(reply) = self.shared.model.health_check_response(&raw)
            && self.send_raw(reply).await.is_err()
        {
            return true;
        }
        let events = match self.shared.model.parse_server_event(raw) {
            Ok(events) => events,
            Err(error) => return !self.emit(Err(Error::from(error))).await,
        };
        for event in events {
            if !self.handle_event(event, tool_tasks).await {
                return true;
            }
        }
        false
    }

    /// Handles one standardized event; returns `false` when the consumer is
    /// gone or sending failed.
    async fn handle_event(
        &mut self,
        event: RealtimeServerEvent,
        tool_tasks: &mut JoinSet<()>,
    ) -> bool {
        let follow_up = match &event {
            RealtimeServerEvent::FunctionCallArgumentsDone {
                call_id,
                name,
                arguments,
                ..
            } => {
                let name = ToolName::new(name.clone());
                self.shared.tool_turn().call_started(call_id, &name);
                Some(ToolCall {
                    call_id: call_id.clone(),
                    name,
                    arguments: arguments.clone(),
                })
            }
            _ => None,
        };
        let response_done = matches!(event, RealtimeServerEvent::ResponseDone { .. });

        if !self.emit(Ok(event)).await {
            return false;
        }
        if let Some(call) = follow_up
            && !self.start_tool_call(call, tool_tasks).await
        {
            return false;
        }
        if response_done && self.shared.tool_turn().response_done() {
            let handle = RealtimeHandle {
                shared: Arc::clone(&self.shared),
            };
            if let Err(error) = handle.request_response().await {
                return self.emit(Err(error)).await;
            }
        }
        true
    }

    /// Starts executing a tool call; returns `false` when the consumer is
    /// gone.
    async fn start_tool_call(&self, call: ToolCall, tool_tasks: &mut JoinSet<()>) -> bool {
        let Some(tool) = self.tools.get(call.name.as_str()).map(Arc::clone) else {
            let available = self.tools.names().cloned().collect();
            return self
                .emit(Err(Error::no_such_tool(call.name, available)))
                .await;
        };
        if !tool.is_executable() {
            // Advertised without an executor: the application answers through
            // `add_tool_output`.
            return true;
        }
        let input: JsonValue = match serde_json::from_str(&call.arguments) {
            Ok(input) => input,
            Err(error) => {
                return self
                    .emit(Err(Error::invalid_tool_input(
                        call.name,
                        call.arguments,
                        error,
                    )))
                    .await;
            }
        };
        let input = match tool.validate_input(&call.name, input) {
            Ok(input) => input,
            Err(error) => {
                return self
                    .emit(Err(Error::invalid_tool_input(
                        call.name,
                        call.arguments,
                        error,
                    )))
                    .await;
            }
        };
        let ctx = ToolContext::new(ToolCallId::new(call.call_id.clone()))
            .with_cancellation(self.shared.cancellation.child_token())
            .with_tools_context(self.tools_context.clone());
        let handle = RealtimeHandle {
            shared: Arc::clone(&self.shared),
        };
        let events = self.events.clone();
        tool_tasks.spawn(execute_tool_call(
            tool,
            input,
            ctx,
            call.call_id,
            handle,
            events,
        ));
        true
    }
}

struct ToolCall {
    call_id: String,
    name: ToolName,
    arguments: String,
}

async fn execute_tool_call(
    tool: Arc<Tool>,
    input: JsonValue,
    ctx: ToolContext,
    call_id: String,
    handle: RealtimeHandle,
    events: mpsc::Sender<RealtimeEvent>,
) {
    let Some(stream) = tool.execute(input, ctx) else {
        return;
    };
    let result = match execute_to_completion(stream, |_| {}).await {
        Ok(output) => handle.add_tool_output(&call_id, &output).await,
        Err(error) => Err(Error::from(error)),
    };
    if let Err(error) = result {
        let _ = events.send(Err(error)).await;
    }
}
