//! Owned, backpressured Live audio driver shared by both audio adapters.
//!
//! Wire behavior derives from the Vercel AI SDK Google audio adapters
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); see the root NOTICE.

mod connection;

use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use ferrin_spec::AudioFormat;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::dynamic::BoxStream;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::StreamError;
use futures_util::SinkExt;
use futures_util::StreamExt;
use serde_json::json;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_util::sync::CancellationToken;

use crate::config::SharedConfig;

pub(crate) const FINISH_GRACE: Duration = Duration::from_secs(1);

pub(crate) fn validate_format(format: &AudioFormat) -> Result<(), ProviderError> {
    if !matches!(format.kind.as_str(), "audio/pcm" | "pcm16")
        || format.rate.is_some_and(|rate| rate != 16_000)
    {
        return Err(InvalidArgumentError::new(
            "input_audio_format",
            "Google Live audio requires 16 kHz signed 16-bit mono PCM",
        )
        .into());
    }
    Ok(())
}

pub(crate) trait Mapper: Send + 'static {
    type Part: Send;

    fn start(&mut self) -> Self::Part;
    fn raw(raw_value: JsonValue) -> Self::Part;
    fn error(error: StreamError) -> Self::Part;
    fn message(&mut self, value: &JsonValue) -> Result<Vec<Self::Part>, Box<StreamError>>;
    fn audio_ended(&mut self);
    fn deadline(&self) -> Option<Instant>;
    fn can_close(&self) -> bool;
    fn complete(&self) -> bool;
    fn finish(&mut self) -> Vec<Self::Part>;
}

struct State<M: Mapper> {
    socket: Option<connection::Socket>,
    audio: Option<BoxStream<'static, Bytes>>,
    remainder: Bytes,
    mapper: M,
    queue: VecDeque<M::Part>,
    cancellation: CancellationToken,
    setup_complete: bool,
    include_raw: bool,
}

pub(crate) struct Options {
    pub(crate) setup: JsonValue,
    pub(crate) audio: BoxStream<'static, Bytes>,
    pub(crate) headers: Headers,
    pub(crate) cancellation: CancellationToken,
    pub(crate) include_raw: bool,
}

pub(crate) async fn start<M: Mapper>(
    config: &SharedConfig,
    model_id: ferrin_spec::ModelId,
    options: Options,
    mut mapper: M,
) -> Result<(BoxStream<'static, M::Part>, ResponseMetadata), ProviderError> {
    let (mut socket, headers) =
        connection::connect(config, &options.headers, &options.cancellation).await?;
    tokio::select! {
        biased;
        () = options.cancellation.cancelled() => return Err(ProviderError::Cancelled),
        result = socket.send(Message::text(json!({"setup": options.setup}).to_string())) => {
            result.map_err(|_| ProviderError::message("could not send Google Live setup"))?;
        }
    }
    let queue = VecDeque::from([mapper.start()]);
    let state = State {
        socket: Some(socket),
        audio: Some(options.audio),
        remainder: Bytes::new(),
        mapper,
        queue,
        cancellation: options.cancellation,
        setup_complete: false,
        include_raw: options.include_raw,
    };
    let stream = futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(part) = state.queue.pop_front() {
                return Some((part, state));
            }
            state.socket.as_ref()?;
            state.advance().await;
        }
    });
    Ok((
        Box::pin(stream),
        ResponseMetadata {
            timestamp: Some(chrono::Utc::now()),
            model_id: Some(model_id),
            headers: Some(headers),
            ..ResponseMetadata::default()
        },
    ))
}

enum Event {
    Cancel,
    Deadline,
    Input(Option<Bytes>),
    Message(Option<Result<Message, tokio_tungstenite::tungstenite::Error>>),
}

impl<M: Mapper> State<M> {
    async fn advance(&mut self) {
        let deadline = self.mapper.deadline();
        let event = {
            let Some(socket) = self.socket.as_mut() else {
                return;
            };
            let audio = &mut self.audio;
            let remainder = &mut self.remainder;
            tokio::select! {
                biased;
                () = self.cancellation.cancelled() => Event::Cancel,
                // Drain ready provider output before evaluating a quiet window;
                // downstream backpressure must not discard buffered transcripts.
                message = socket.next() => Event::Message(message),
                () = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                } => Event::Deadline,
                chunk = async {
                    if !remainder.is_empty() { return Some(std::mem::take(remainder)); }
                    match audio {
                        Some(audio) => audio.next().await,
                        None => std::future::pending().await,
                    }
                }, if self.setup_complete => Event::Input(chunk),
            }
        };
        match event {
            Event::Cancel => {
                let mut error = StreamError::new("operation cancelled");
                error.error_type = Some("cancelled".to_owned());
                self.fail(error);
            }
            Event::Deadline => self.finish(),
            Event::Input(chunk) => self.send_audio(chunk).await,
            Event::Message(Some(Ok(Message::Text(text)))) => self.message(text.as_bytes()),
            Event::Message(Some(Ok(Message::Binary(bytes)))) => self.message(&bytes),
            Event::Message(Some(Ok(Message::Ping(_)))) => {
                // tungstenite queues the matching Pong while reading the Ping.
                if let Some(socket) = self.socket.as_mut() {
                    let result = tokio::select! {
                        () = self.cancellation.cancelled() => Err(()),
                        result = socket.flush() => result.map_err(|_| ()),
                    };
                    if result.is_err() {
                        self.fail(StreamError::new("Google Live connection interrupted"));
                    }
                }
            }
            Event::Message(Some(Ok(Message::Close(frame)))) => {
                let normal = frame
                    .as_ref()
                    .is_none_or(|frame| frame.code == CloseCode::Normal);
                if normal && self.mapper.can_close() {
                    self.finish();
                } else {
                    self.fail(StreamError::new(
                        "Google Live WebSocket closed before completion",
                    ));
                }
            }
            Event::Message(None | Some(Err(_))) => {
                self.fail(StreamError::new(
                    "Google Live WebSocket ended before completion",
                ));
            }
            Event::Message(Some(Ok(Message::Pong(_) | Message::Frame(_)))) => {}
        }
    }

    async fn send_audio(&mut self, chunk: Option<Bytes>) {
        let body = if let Some(mut chunk) = chunk {
            if chunk.is_empty() {
                return;
            }
            // Bound encoded writes even if a caller supplies a large chunk.
            let data = chunk.split_to(chunk.len().min(8192));
            self.remainder = chunk;
            json!({"realtimeInput": {"audio": {
                "data": base64::engine::general_purpose::STANDARD.encode(data),
                "mimeType": "audio/pcm;rate=16000",
            }}})
        } else {
            self.audio = None;
            json!({"realtimeInput": {"audioStreamEnd": true}})
        };
        let Some(socket) = self.socket.as_mut() else {
            return;
        };
        let result = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(ProviderError::Cancelled),
            result = socket.send(Message::text(body.to_string())) => {
                result.map_err(|_| ProviderError::message("could not send Google Live audio"))
            }
        };
        match result {
            Err(error) => self.fail(StreamError::from_provider_error(&error)),
            Ok(()) if self.audio.is_none() => self.mapper.audio_ended(),
            Ok(()) => {}
        }
    }

    fn message(&mut self, bytes: &[u8]) {
        let Ok(value) = serde_json::from_slice::<JsonValue>(bytes) else {
            self.fail(StreamError::new("Google Live returned invalid JSON"));
            return;
        };
        if !value.is_object() {
            self.fail(StreamError::new("Google Live message must be an object"));
            return;
        }
        if self.include_raw {
            self.queue.push_back(M::raw(value.clone()));
        }
        if value.get("error").is_some() {
            self.fail(StreamError::new("Google Live API reported an error"));
            return;
        }
        if value.get("setupComplete").is_some_and(JsonValue::is_object) {
            self.setup_complete = true;
        }
        match self.mapper.message(&value) {
            Ok(parts) => self.queue.extend(parts),
            Err(error) => {
                self.fail(*error);
                return;
            }
        }
        if self.mapper.complete() {
            self.finish();
        }
    }

    fn release(&mut self) {
        self.socket = None;
        self.audio = None;
        self.remainder = Bytes::new();
    }

    fn finish(&mut self) {
        self.queue.extend(self.mapper.finish());
        self.release();
    }

    fn fail(&mut self, mut error: StreamError) {
        if self.cancellation.is_cancelled() {
            error = StreamError::new("operation cancelled");
            error.error_type = Some("cancelled".to_owned());
        }
        self.queue.push_back(M::error(error));
        self.release();
    }
}
