//! Realtime (WebSocket) sessions.
//!
//! A [`RealtimeSession`] connects to a provider's realtime endpoint through
//! the provider's [`RealtimeModel`](ferrin_spec::RealtimeModel): the model
//! issues the client secret, describes the WebSocket handshake and
//! translates between wire messages and the standardized
//! [`RealtimeClientEvent`] / [`RealtimeServerEvent`] sets. The session owns
//! the connection, forwards server events as a stream, and executes local
//! function tools when the model calls them.
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §9.

mod session;
mod tools;

use std::future::IntoFuture;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use ferrin_spec::JsonValue;
use ferrin_spec::RealtimeModelRef;
use ferrin_spec::realtime_model::ClientSecretOptions;
use ferrin_tool::ToolSet;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub use ferrin_spec::realtime_model::ClientSecret;
pub use ferrin_spec::realtime_model::ConversationItem;
pub use ferrin_spec::realtime_model::ConversationRole;
pub use ferrin_spec::realtime_model::Modality;
pub use ferrin_spec::realtime_model::RealtimeClientEvent;
pub use ferrin_spec::realtime_model::RealtimeServerEvent;
pub use ferrin_spec::realtime_model::RealtimeSessionConfig;
pub use ferrin_spec::realtime_model::RealtimeToolDefinition;
pub use ferrin_spec::realtime_model::ResponseCreateOptions;
pub use ferrin_spec::realtime_model::TranscriptionConfig;
pub use ferrin_spec::realtime_model::TurnDetection;
pub use ferrin_spec::realtime_model::TurnDetectionKind;
pub use session::RealtimeHandle;
pub use tools::realtime_tool_definitions;

use crate::error::Error;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;

/// Default capacity of the server event buffer.
const DEFAULT_EVENT_BUFFER: usize = 256;

/// How long [`RealtimeSession::close`] waits for the connection task.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Items of the event stream: standardized server events, or local failures
/// (transport errors, unparsable messages, tool execution errors).
pub type RealtimeEvent = Result<RealtimeServerEvent, Error>;

/// Starts building a realtime session with `model`.
///
/// Call [`RealtimeSessionBuilder::connect`] (or `.await` the builder) to open
/// the connection.
#[must_use]
pub fn realtime_session(model: impl Into<RealtimeModelRef>) -> RealtimeSessionBuilder {
    RealtimeSessionBuilder {
        model: model.into(),
        client_secret: None,
        expires_after_seconds: None,
        config: RealtimeSessionConfig::default(),
        tools: ToolSet::new(),
        tools_context: None,
        cancellation: CancellationToken::new(),
        event_buffer: DEFAULT_EVENT_BUFFER,
    }
}

/// Configuration of a realtime session before it connects.
#[derive(Debug)]
pub struct RealtimeSessionBuilder {
    model: RealtimeModelRef,
    client_secret: Option<ClientSecret>,
    expires_after_seconds: Option<u64>,
    config: RealtimeSessionConfig,
    tools: ToolSet,
    tools_context: Option<JsonValue>,
    cancellation: CancellationToken,
    event_buffer: usize,
}

impl RealtimeSessionBuilder {
    /// Uses an existing client secret instead of creating one through the
    /// model.
    #[must_use]
    pub fn client_secret(mut self, secret: ClientSecret) -> Self {
        self.client_secret = Some(secret);
        self
    }

    /// Requested lifetime of the client secret created on connect.
    #[must_use]
    pub fn expires_after_seconds(mut self, seconds: u64) -> Self {
        self.expires_after_seconds = Some(seconds);
        self
    }

    /// Sets the initial session configuration. Tool definitions derived from
    /// [`tools`](Self::tools) are appended to `config.tools` on connect.
    #[must_use]
    pub fn config(mut self, config: RealtimeSessionConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the system instructions of the session.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.config.instructions = Some(instructions.into());
        self
    }

    /// Sets the voice used for audio output.
    #[must_use]
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.config.voice = Some(voice.into());
        self
    }

    /// Local tools. Function and dynamic tools are advertised to the model;
    /// executable ones run inside the session and their outputs are sent
    /// back automatically. Tools without an executor are advertised only;
    /// the application answers them with [`RealtimeHandle::add_tool_output`].
    #[must_use]
    pub fn tools(mut self, tools: ToolSet) -> Self {
        self.tools = tools;
        self
    }

    /// Context passed to dynamic tool descriptions and executions.
    #[must_use]
    pub fn tools_context(mut self, context: JsonValue) -> Self {
        self.tools_context = Some(context);
        self
    }

    /// Cancellation token; cancelling it closes the session.
    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Capacity of the server event buffer (default 256). Reading from the
    /// connection pauses while the buffer is full.
    #[must_use]
    pub fn event_buffer(mut self, capacity: usize) -> Self {
        self.event_buffer = capacity.max(1);
        self
    }

    /// Opens the connection and sends the initial `session-update`.
    ///
    /// # Errors
    ///
    /// Fails when the client secret cannot be created, the WebSocket
    /// handshake fails, a tool context is invalid or the initial event
    /// cannot be serialized.
    pub async fn connect(self) -> Result<RealtimeSession, Error> {
        let model = resolve_model(&self.model, ProviderRegistry::realtime_model)?;
        let mut config = self.config;
        let definitions =
            realtime_tool_definitions(&self.tools, self.tools_context.as_ref()).await?;
        config.tools.extend(definitions);

        let secret = match self.client_secret {
            Some(secret) => secret,
            None => model
                .do_create_client_secret(ClientSecretOptions {
                    expires_after_seconds: self.expires_after_seconds,
                    session_config: Some(config.clone()),
                })
                .await
                .map_err(Error::from)?,
        };

        let (events_tx, events_rx) = mpsc::channel(self.event_buffer);
        let mut tasks = JoinSet::new();
        let handle = session::start(
            session::StartOptions {
                model,
                secret,
                config,
                tools: Arc::new(self.tools),
                tools_context: self.tools_context,
                cancellation: self.cancellation,
                events: events_tx,
            },
            &mut tasks,
        )
        .await?;
        Ok(RealtimeSession {
            handle,
            events: events_rx,
            tasks,
        })
    }
}

impl IntoFuture for RealtimeSessionBuilder {
    type Output = Result<RealtimeSession, Error>;
    type IntoFuture = futures_util::future::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.connect())
    }
}

/// An open realtime session.
///
/// The session is a [`Stream`] of [`RealtimeEvent`]s. Sending happens through
/// [`RealtimeSession::send`] or a cloned [`RealtimeHandle`], which stays
/// usable while the session is being polled from another task. The stream
/// ends when the connection closes.
#[derive(Debug)]
pub struct RealtimeSession {
    handle: RealtimeHandle,
    events: mpsc::Receiver<RealtimeEvent>,
    tasks: JoinSet<()>,
}

impl RealtimeSession {
    /// Returns a cloneable handle for sending events and closing the session.
    #[must_use]
    pub fn handle(&self) -> RealtimeHandle {
        self.handle.clone()
    }

    /// Sends a client event.
    ///
    /// # Errors
    ///
    /// Fails when the event cannot be serialized or the session is closed.
    pub async fn send(&self, event: RealtimeClientEvent) -> Result<(), Error> {
        self.handle.send(event).await
    }

    /// Sends a user text message and requests a response.
    ///
    /// # Errors
    ///
    /// See [`RealtimeSession::send`].
    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), Error> {
        self.handle.send_text(text).await
    }

    /// Submits the output of a tool call the application executed itself.
    ///
    /// # Errors
    ///
    /// See [`RealtimeSession::send`].
    pub async fn add_tool_output(&self, call_id: &str, output: &JsonValue) -> Result<(), Error> {
        self.handle.add_tool_output(call_id, output).await
    }

    /// Returns the next event, or `None` once the connection is closed.
    pub async fn next_event(&mut self) -> Option<RealtimeEvent> {
        self.events.recv().await
    }

    /// Borrows the session as a stream of events.
    pub fn events(&mut self) -> impl Stream<Item = RealtimeEvent> + Send + '_ {
        self
    }

    /// Returns `true` once the connection task has finished.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Closes the connection and waits (bounded) for the connection task.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when the connection task does not finish
    /// within five seconds; the task is aborted in that case.
    pub async fn close(mut self) -> Result<(), Error> {
        self.handle.close();
        let started = tokio::time::Instant::now();
        let deadline = started + CLOSE_TIMEOUT;
        loop {
            match tokio::time::timeout_at(deadline, self.tasks.join_next()).await {
                Ok(None) => return Ok(()),
                Ok(Some(_)) => {}
                Err(_) => {
                    self.tasks.abort_all();
                    return Err(Error::Timeout {
                        scope: crate::timeout::TimeoutScope::Total,
                        elapsed: started.elapsed(),
                    });
                }
            }
        }
    }
}

impl Stream for RealtimeSession {
    type Item = RealtimeEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.events.poll_recv(cx)
    }
}
