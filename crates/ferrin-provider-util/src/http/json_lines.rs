//! Bounded incremental newline-delimited JSON parsing.

use std::marker::PhantomData;

use bytes::Buf;
use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::error::EmptyResponseBodyError;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use super::BodyStream;
use super::Handled;
use super::HttpResponse;
use super::ParseResult;
use super::ResponseContext;
use super::ResponseHandler;
use super::ResponseHead;
use super::TransportError;
use super::TransportErrorKind;
use super::handlers::stream_error;
use super::parse_json_chunk;

/// Decodes a newline-delimited JSON body into chunks of type `T`.
pub struct JsonLinesResponseHandler<T> {
    max_line_bytes: usize,
    _marker: PhantomData<fn() -> T>,
}

impl<T> std::fmt::Debug for JsonLinesResponseHandler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonLinesResponseHandler")
            .field("max_line_bytes", &self.max_line_bytes)
            .finish()
    }
}

impl<T> Default for JsonLinesResponseHandler<T> {
    fn default() -> Self {
        Self {
            max_line_bytes: 16 * 1024 * 1024,
            _marker: PhantomData,
        }
    }
}

impl<T> JsonLinesResponseHandler<T> {
    /// Sets the maximum bytes before each newline, including a trailing CR.
    ///
    /// The default is 16 MiB. Excess size terminates the stream with one error
    /// and releases the response body, including an unterminated final line.
    #[must_use]
    pub fn with_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.max_line_bytes = max_line_bytes;
        self
    }
}

struct State {
    body: Option<BodyStream>,
    chunk: Bytes,
    line: Vec<u8>,
    context: ResponseContext,
    head: ResponseHead,
    max_line_bytes: usize,
}

impl State {
    fn finish(&mut self) {
        self.body = None;
        self.chunk = Bytes::new();
        self.line = Vec::new();
    }

    fn error<T>(&mut self, error: TransportError) -> ParseResult<T> {
        self.finish();
        ParseResult::Err {
            error: stream_error(&self.context, &self.head, error),
            raw: None,
        }
    }

    fn take_line<T: DeserializeOwned>(&mut self) -> Option<ParseResult<T>> {
        let line = std::mem::take(&mut self.line);
        let text = String::from_utf8_lossy(&line);
        if text.trim().is_empty() {
            None
        } else {
            Some(parse_json_chunk::<T>(text.trim_end_matches('\r')))
        }
    }

    async fn next<T: DeserializeOwned>(&mut self) -> Option<ParseResult<T>> {
        loop {
            if !self.chunk.is_empty() {
                let newline = self.chunk.iter().position(|byte| *byte == b'\n');
                let count = newline.unwrap_or(self.chunk.len());
                if count > self.max_line_bytes.saturating_sub(self.line.len()) {
                    return Some(self.error(TransportError::new(
                        TransportErrorKind::BodyTooLarge,
                        format!(
                            "json line exceeds the limit of {} bytes",
                            self.max_line_bytes
                        ),
                    )));
                }
                self.line.extend_from_slice(&self.chunk.split_to(count));
                if newline.is_some() {
                    self.chunk.advance(1);
                    if let Some(item) = self.take_line() {
                        return Some(item);
                    }
                }
                continue;
            }
            let Some(body) = &mut self.body else {
                return self.take_line();
            };
            match body.next().await {
                Some(Ok(chunk)) => self.chunk = chunk,
                Some(Err(error)) => return Some(self.error(error)),
                None => self.body = None,
            }
        }
    }
}

impl<T: DeserializeOwned + Send + 'static> ResponseHandler<BoxStream<'static, ParseResult<T>>>
    for JsonLinesResponseHandler<T>
{
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<BoxStream<'static, ParseResult<T>>>, ProviderError>>
    {
        let max_line_bytes = self.max_line_bytes;
        Box::pin(async move {
            if response.headers.get_str("content-length") == Some("0") {
                return Err(EmptyResponseBodyError::new().into());
            }
            let state = State {
                head: response.head(),
                body: Some(response.body),
                chunk: Bytes::new(),
                line: Vec::new(),
                context,
                max_line_bytes,
            };
            let stream = futures_util::stream::unfold(state, |mut state| async move {
                state.next().await.map(|item| (item, state))
            });
            let value: BoxStream<'static, ParseResult<T>> = Box::pin(stream);
            Ok(Handled {
                value,
                raw: None,
                headers: response.headers,
            })
        })
    }
}

/// Builds a [`JsonLinesResponseHandler`].
#[must_use]
pub fn json_lines_response_handler<T>() -> JsonLinesResponseHandler<T> {
    JsonLinesResponseHandler::default()
}
