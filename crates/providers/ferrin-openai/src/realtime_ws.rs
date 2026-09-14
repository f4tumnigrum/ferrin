//! WebSocket plumbing shared by the streaming transcription and speech
//! translation models (`realtime` feature).
//!
//! The bearer token travels in the `openai-insecure-api-key.<token>`
//! WebSocket sub-protocol; the `authorization` header is not sent.

use std::collections::VecDeque;

use base64::Engine;
use bytes::Bytes;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use futures_util::SinkExt;
use futures_util::StreamExt;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use http::HeaderName;
use http::HeaderValue;
use http::header::SEC_WEBSOCKET_PROTOCOL;
use serde_json::json;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_util::sync::CancellationToken;
use url::Url;

/// An open WebSocket.
pub(crate) type WsSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Extracts the token of a `Bearer <token>` header value.
fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.trim().split_once(char::is_whitespace)?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
}

/// Sub-protocols carrying the API key.
pub(crate) fn protocols(token: Option<&str>) -> Vec<String> {
    let mut protocols = vec!["realtime".to_owned()];
    if let Some(token) = token {
        protocols.push(format!("openai-insecure-api-key.{token}"));
    }
    protocols
}

fn connect_error(url: &Url, error: tungstenite::Error) -> ProviderError {
    match error {
        tungstenite::Error::Http(response) => {
            let status = http::StatusCode::from_u16(response.status().as_u16())
                .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
            let body = response
                .body()
                .as_ref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            ApiCallError::new(
                format!("WebSocket handshake failed with status {status}"),
                url.clone(),
            )
            .with_status(status)
            .with_response(Headers::new(), body)
            .into()
        }
        other => ApiCallError::new(format!("WebSocket connection failed: {other}"), url.clone())
            .retryable(false)
            .into(),
    }
}

/// Opens a WebSocket to `url`, moving the bearer token of `headers` into the
/// sub-protocol list.
///
/// # Errors
///
/// Returns [`ProviderError::ApiCall`] when the handshake fails and
/// [`ProviderError::Cancelled`] when `cancellation` fires first.
pub(crate) async fn connect(
    url: &Url,
    headers: &Headers,
    cancellation: &CancellationToken,
) -> Result<WsSocket, ProviderError> {
    let mut request = url.as_str().into_client_request().map_err(|error| {
        ApiCallError::new(format!("invalid WebSocket URL: {error}"), url.clone()).retryable(false)
    })?;
    let token = headers.get_str("authorization").and_then(bearer_token);
    let protocol_list = protocols(token).join(", ");
    for (name, value) in headers.iter_str() {
        if name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            request.headers_mut().insert(name, value);
        }
    }
    let protocol_value = HeaderValue::from_str(&protocol_list).map_err(|error| {
        ApiCallError::new(
            format!("invalid WebSocket protocol header: {error}"),
            url.clone(),
        )
        .retryable(false)
    })?;
    request
        .headers_mut()
        .insert(SEC_WEBSOCKET_PROTOCOL, protocol_value);
    let connect = connect_async_tls_with_config(request, None, false, None);
    let (socket, _response) = tokio::select! {
        result = connect => result.map_err(|error| connect_error(url, error))?,
        () = cancellation.cancelled() => return Err(ProviderError::Cancelled),
    };
    Ok(socket)
}

/// Whether the stream continues after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WsFlow {
    /// Keep reading.
    Continue,
    /// The stream is complete; close the socket.
    Finish,
}

/// Maps server events of one WebSocket session to stream parts.
pub(crate) trait WsMachine: Send + 'static {
    /// Emitted part type.
    type Part: Send + 'static;

    /// Parts emitted once the session update was sent.
    fn start(&mut self) -> Vec<Self::Part>;

    /// Wraps a raw server event.
    fn raw(&self, raw: JsonValue) -> Self::Part;

    /// Handles a server event.
    fn handle(&mut self, event: &JsonValue, parts: &mut Vec<Self::Part>) -> WsFlow;

    /// The socket closed before [`WsFlow::Finish`].
    fn closed(&mut self, parts: &mut Vec<Self::Part>);

    /// A transport or protocol error ended the session.
    fn failed(&mut self, error: ProviderError, parts: &mut Vec<Self::Part>);
}

/// Parameters of [`ws_stream`].
pub(crate) struct WsStreamParams<M> {
    /// Open socket.
    pub(crate) socket: WsSocket,
    /// First message sent after opening.
    pub(crate) session_update: JsonValue,
    /// Audio chunks to append.
    pub(crate) audio: BoxStream<'static, Bytes>,
    /// `type` of the append event.
    pub(crate) append_event: &'static str,
    /// Message sent after the last audio chunk.
    pub(crate) commit_event: JsonValue,
    /// Emit raw events.
    pub(crate) include_raw: bool,
    /// Cancellation.
    pub(crate) cancellation: CancellationToken,
    /// Event mapper.
    pub(crate) machine: M,
}

impl<M> std::fmt::Debug for WsStreamParams<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsStreamParams")
            .field("append_event", &self.append_event)
            .field("include_raw", &self.include_raw)
            .finish_non_exhaustive()
    }
}

struct Driver<M: WsMachine> {
    sink: SplitSink<WsSocket, Message>,
    stream: SplitStream<WsSocket>,
    audio: Option<BoxStream<'static, Bytes>>,
    pending: VecDeque<M::Part>,
    machine: M,
    include_raw: bool,
    cancellation: CancellationToken,
    finished: bool,
    started: bool,
    session_update: Option<JsonValue>,
    append_event: &'static str,
    commit_event: Option<JsonValue>,
}

enum Wake {
    Cancelled,
    Audio(Option<Bytes>),
    Frame(Option<Result<Message, tungstenite::Error>>),
}

async fn next_audio(audio: &mut Option<BoxStream<'static, Bytes>>) -> Option<Bytes> {
    match audio.as_mut() {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

impl<M: WsMachine> Driver<M> {
    async fn send(&mut self, value: &JsonValue) -> bool {
        match self.sink.send(Message::text(value.to_string())).await {
            Ok(()) => true,
            Err(error) => {
                self.fail(ProviderError::other(error));
                false
            }
        }
    }

    fn fail(&mut self, error: ProviderError) {
        let mut parts = Vec::new();
        self.machine.failed(error, &mut parts);
        self.pending.extend(parts);
        self.finished = true;
    }

    async fn close(&mut self) {
        let frame = CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        };
        let _ = self.sink.send(Message::Close(Some(frame))).await;
    }

    fn handle_text(&mut self, text: &str) -> WsFlow {
        let Ok(event) = serde_json::from_str::<JsonValue>(text) else {
            return WsFlow::Continue;
        };
        if self.include_raw {
            self.pending.push_back(self.machine.raw(event.clone()));
        }
        let mut parts = Vec::new();
        let flow = self.machine.handle(&event, &mut parts);
        self.pending.extend(parts);
        flow
    }

    async fn step(&mut self) {
        if !self.started {
            self.started = true;
            let parts = self.machine.start();
            self.pending.extend(parts);
            if let Some(update) = self.session_update.take() {
                self.send(&update).await;
            }
            return;
        }
        let audio_pending = self.audio.is_some();
        let wake = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Wake::Cancelled,
            chunk = next_audio(&mut self.audio), if audio_pending => Wake::Audio(chunk),
            frame = self.stream.next() => Wake::Frame(frame),
        };
        match wake {
            Wake::Cancelled => {
                self.close().await;
                self.fail(ProviderError::Cancelled);
            }
            Wake::Audio(Some(bytes)) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let event = json!({"type": self.append_event, "audio": encoded});
                self.send(&event).await;
            }
            Wake::Audio(None) => {
                self.audio = None;
                if let Some(commit) = self.commit_event.take() {
                    self.send(&commit).await;
                }
            }
            Wake::Frame(Some(Ok(Message::Text(text)))) => {
                if self.handle_text(text.as_str()) == WsFlow::Finish {
                    self.close().await;
                    self.finished = true;
                }
            }
            Wake::Frame(Some(Ok(Message::Binary(bytes)))) => {
                if let Ok(text) = std::str::from_utf8(&bytes)
                    && self.handle_text(text) == WsFlow::Finish
                {
                    self.close().await;
                    self.finished = true;
                }
            }
            Wake::Frame(Some(Ok(Message::Close(_))) | None) => {
                let mut parts = Vec::new();
                self.machine.closed(&mut parts);
                self.pending.extend(parts);
                self.finished = true;
            }
            Wake::Frame(Some(Ok(_))) => {}
            Wake::Frame(Some(Err(error))) => self.fail(ProviderError::other(error)),
        }
    }
}

/// Drives a WebSocket session as a stream of parts.
pub(crate) fn ws_stream<M: WsMachine>(params: WsStreamParams<M>) -> BoxStream<'static, M::Part> {
    let (sink, stream) = params.socket.split();
    let driver = Driver {
        sink,
        stream,
        audio: Some(params.audio),
        pending: VecDeque::new(),
        machine: params.machine,
        include_raw: params.include_raw,
        cancellation: params.cancellation,
        finished: false,
        started: false,
        session_update: Some(params.session_update),
        append_event: params.append_event,
        commit_event: Some(params.commit_event),
    };
    Box::pin(futures_util::stream::unfold(
        driver,
        |mut driver| async move {
            loop {
                if let Some(part) = driver.pending.pop_front() {
                    return Some((part, driver));
                }
                if driver.finished {
                    return None;
                }
                driver.step().await;
            }
        },
    ))
}
