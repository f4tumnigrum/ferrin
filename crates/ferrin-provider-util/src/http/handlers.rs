//! Response handlers: turn an [`HttpResponse`] into a typed value or error.

use std::marker::PhantomData;
use std::sync::Arc;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::EmptyResponseBodyError;
use ferrin_spec::error::JsonParseError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TypeValidationError;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use url::Url;

use super::body::DEFAULT_MAX_RESPONSE_BYTES;
use super::body::read_body;
use super::transport::BodyStream;
use super::transport::HttpResponse;
use super::transport::ResponseHead;
use super::transport::TransportError;
use crate::sse;
use crate::sse::SseStreamError;

/// Request information passed to handlers for error reporting.
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// Request URL.
    pub url: Url,
    /// JSON rendering of the request body, if any.
    pub request_body: Option<Arc<JsonValue>>,
}

impl ResponseContext {
    /// Creates a context.
    #[must_use]
    pub fn new(url: Url, request_body: Option<JsonValue>) -> Self {
        Self {
            url,
            request_body: request_body.map(Arc::new),
        }
    }

    /// Starts an [`ApiCallError`] for this request.
    #[must_use]
    pub fn api_error(&self, message: impl Into<String>) -> ApiCallError {
        let mut error = ApiCallError::new(message, self.url.clone());
        if let Some(body) = &self.request_body {
            error = error.with_request_body((**body).clone());
        }
        error
    }
}

/// Output of a handler.
#[derive(Debug)]
pub struct Handled<T> {
    /// The typed value.
    pub value: T,
    /// The raw JSON body when the handler parsed JSON.
    pub raw: Option<JsonValue>,
    /// Response headers.
    pub headers: Headers,
}

/// Interprets a response.
///
/// Handlers are cheap to clone into the returned future; they must not
/// borrow from `self` across the await.
pub trait ResponseHandler<T>: Send + Sync {
    /// Consumes the response.
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<T>, ProviderError>>;
}

/// Success and failure handlers for one request.
pub struct ResponseHandlers<T> {
    /// Handles 2xx responses.
    pub success: Box<dyn ResponseHandler<T>>,
    /// Handles non-2xx responses, producing the error to return.
    pub failure: Box<dyn ResponseHandler<ProviderError>>,
}

impl<T> std::fmt::Debug for ResponseHandlers<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseHandlers").finish_non_exhaustive()
    }
}

impl<T> ResponseHandlers<T> {
    /// Pairs a success handler with a failure handler.
    pub fn new(
        success: impl ResponseHandler<T> + 'static,
        failure: impl ResponseHandler<ProviderError> + 'static,
    ) -> Self {
        Self {
            success: Box::new(success),
            failure: Box::new(failure),
        }
    }
}

/// Result of parsing one streamed chunk.
#[derive(Debug)]
pub enum ParseResult<T> {
    /// The chunk parsed and validated.
    Ok {
        /// Typed value.
        value: T,
        /// Raw JSON value, for `include_raw_chunks`.
        raw: JsonValue,
    },
    /// The chunk could not be read, parsed or validated.
    Err {
        /// The failure.
        error: ProviderError,
        /// Raw text of the chunk when it was received.
        raw: Option<String>,
    },
}

impl<T> ParseResult<T> {
    /// Converts into a `Result`, dropping the raw payloads.
    ///
    /// # Errors
    ///
    /// Returns the parse error.
    pub fn into_result(self) -> Result<T, ProviderError> {
        match self {
            Self::Ok { value, .. } => Ok(value),
            Self::Err { error, .. } => Err(error),
        }
    }
}

/// Parses `text` as JSON and deserializes it into `T`.
#[must_use]
pub fn parse_json_chunk<T: DeserializeOwned>(text: &str) -> ParseResult<T> {
    match serde_json::from_str::<JsonValue>(text) {
        Ok(raw) => match serde_json::from_value::<T>(raw.clone()) {
            Ok(value) => ParseResult::Ok { value, raw },
            Err(error) => ParseResult::Err {
                error: TypeValidationError::new(raw, error).into(),
                raw: Some(text.to_owned()),
            },
        },
        Err(error) => ParseResult::Err {
            error: JsonParseError::new(text, error).into(),
            raw: Some(text.to_owned()),
        },
    }
}

async fn read_text(
    context: &ResponseContext,
    head: &ResponseHead,
    body: BodyStream,
    max_bytes: u64,
) -> Result<String, ProviderError> {
    let bytes = read_body(&head.headers, body, max_bytes)
        .await
        .map_err(|error| body_error(context, head, error))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn body_error(
    context: &ResponseContext,
    head: &ResponseHead,
    error: TransportError,
) -> ProviderError {
    if error.is_cancelled() {
        return ProviderError::Cancelled;
    }
    let retryable = error.is_retryable();
    context
        .api_error(format!("failed to read response body: {}", error.message))
        .with_status(head.status)
        .with_response(head.headers.clone(), None)
        .retryable(retryable)
        .with_cause(error)
        .into()
}

/// Parses a JSON body into `T`, keeping the raw value.
pub struct JsonResponseHandler<T> {
    max_bytes: u64,
    _marker: PhantomData<fn() -> T>,
}

impl<T> std::fmt::Debug for JsonResponseHandler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonResponseHandler")
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

impl<T> JsonResponseHandler<T> {
    /// Creates the handler with the default body limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            _marker: PhantomData,
        }
    }

    /// Sets the body limit.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl<T> Default for JsonResponseHandler<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DeserializeOwned + Send + 'static> ResponseHandler<T> for JsonResponseHandler<T> {
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<T>, ProviderError>> {
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            let head = response.head();
            let text = read_text(&context, &head, response.body, max_bytes).await?;
            match parse_json_chunk::<T>(&text) {
                ParseResult::Ok { value, raw } => Ok(Handled {
                    value,
                    raw: Some(raw),
                    headers: head.headers,
                }),
                ParseResult::Err { error, .. } => Err(context
                    .api_error("invalid JSON response")
                    .with_status(head.status)
                    .with_response(head.headers, Some(text))
                    .with_cause(error)
                    .into()),
            }
        })
    }
}

/// Parses a JSON body into `T`.
#[must_use]
pub fn json_response_handler<T>() -> JsonResponseHandler<T> {
    JsonResponseHandler::new()
}

/// Reads the body as text.
#[derive(Debug, Clone)]
pub struct TextResponseHandler {
    max_bytes: u64,
}

impl ResponseHandler<String> for TextResponseHandler {
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<String>, ProviderError>> {
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            let head = response.head();
            let text = read_text(&context, &head, response.body, max_bytes).await?;
            Ok(Handled {
                value: text,
                raw: None,
                headers: head.headers,
            })
        })
    }
}

/// Reads the body as text.
#[must_use]
pub fn text_response_handler() -> TextResponseHandler {
    TextResponseHandler {
        max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
    }
}

type ToMessage<E> = Arc<dyn Fn(&E) -> String + Send + Sync>;
type IsRetryable<E> = Arc<dyn Fn(&ResponseHead, Option<&E>) -> bool + Send + Sync>;

/// Parses an error body into `E` and builds an `ApiCallError` from it.
///
/// Empty or unparsable bodies produce an error whose message is the status
/// reason phrase.
pub struct JsonErrorResponseHandler<E> {
    to_message: ToMessage<E>,
    is_retryable: Option<IsRetryable<E>>,
    max_bytes: u64,
}

impl<E> std::fmt::Debug for JsonErrorResponseHandler<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonErrorResponseHandler")
            .field("max_bytes", &self.max_bytes)
            .finish_non_exhaustive()
    }
}

impl<E> Clone for JsonErrorResponseHandler<E> {
    fn clone(&self) -> Self {
        Self {
            to_message: Arc::clone(&self.to_message),
            is_retryable: self.is_retryable.clone(),
            max_bytes: self.max_bytes,
        }
    }
}

impl<E> JsonErrorResponseHandler<E> {
    /// Creates the handler with a message extractor.
    pub fn new(to_message: impl Fn(&E) -> String + Send + Sync + 'static) -> Self {
        Self {
            to_message: Arc::new(to_message),
            is_retryable: None,
            max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }

    /// Overrides retryability based on the response and parsed error.
    #[must_use]
    pub fn with_is_retryable(
        mut self,
        is_retryable: impl Fn(&ResponseHead, Option<&E>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.is_retryable = Some(Arc::new(is_retryable));
        self
    }

    /// Sets the body limit.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl<E: DeserializeOwned + Send + Sync + 'static> ResponseHandler<ProviderError>
    for JsonErrorResponseHandler<E>
{
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<ProviderError>, ProviderError>> {
        let this = self.clone();
        Box::pin(async move {
            let head = response.head();
            let text = read_text(&context, &head, response.body, this.max_bytes).await?;
            let status_message = head
                .status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_owned();
            let (message, data, parsed) = if text.trim().is_empty() {
                (status_message, None, None)
            } else {
                match serde_json::from_str::<JsonValue>(&text) {
                    Ok(raw) => match serde_json::from_value::<E>(raw.clone()) {
                        Ok(parsed) => ((this.to_message)(&parsed), Some(raw), Some(parsed)),
                        Err(_) => (status_message, None, None),
                    },
                    Err(_) => (status_message, None, None),
                }
            };
            let mut error = context
                .api_error(message)
                .with_status(head.status)
                .with_response(head.headers.clone(), Some(text));
            if let Some(data) = data {
                error = error.with_data(data);
            }
            if let Some(is_retryable) = &this.is_retryable {
                error = error.retryable(is_retryable(&head, parsed.as_ref()));
            }
            Ok(Handled {
                value: error.into(),
                raw: None,
                headers: head.headers,
            })
        })
    }
}

/// Builds a [`JsonErrorResponseHandler`].
pub fn json_error_response_handler<E>(
    to_message: impl Fn(&E) -> String + Send + Sync + 'static,
) -> JsonErrorResponseHandler<E> {
    JsonErrorResponseHandler::new(to_message)
}

/// Produces an `ApiCallError` from the status code and body text.
#[derive(Debug, Clone)]
pub struct StatusCodeErrorResponseHandler {
    max_bytes: u64,
}

impl ResponseHandler<ProviderError> for StatusCodeErrorResponseHandler {
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<ProviderError>, ProviderError>> {
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            let head = response.head();
            let text = read_text(&context, &head, response.body, max_bytes).await?;
            let error = context
                .api_error(head.status.canonical_reason().unwrap_or("request failed"))
                .with_status(head.status)
                .with_response(head.headers.clone(), Some(text));
            Ok(Handled {
                value: error.into(),
                raw: None,
                headers: head.headers,
            })
        })
    }
}

/// Builds a [`StatusCodeErrorResponseHandler`].
#[must_use]
pub fn status_code_error_response_handler() -> StatusCodeErrorResponseHandler {
    StatusCodeErrorResponseHandler {
        max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
    }
}

/// Reads the whole body as bytes.
#[derive(Debug, Clone)]
pub struct BinaryResponseHandler {
    max_bytes: u64,
}

impl BinaryResponseHandler {
    /// Sets the body limit.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl ResponseHandler<Bytes> for BinaryResponseHandler {
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<Bytes>, ProviderError>> {
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            let head = response.head();
            let bytes = read_body(&head.headers, response.body, max_bytes)
                .await
                .map_err(|error| body_error(&context, &head, error))?;
            Ok(Handled {
                value: bytes,
                raw: None,
                headers: head.headers,
            })
        })
    }
}

/// Builds a [`BinaryResponseHandler`].
#[must_use]
pub fn binary_response_handler() -> BinaryResponseHandler {
    BinaryResponseHandler {
        max_bytes: DEFAULT_MAX_RESPONSE_BYTES,
    }
}

/// Passes the body stream through.
#[derive(Debug, Clone, Default)]
pub struct BinaryStreamResponseHandler;

impl ResponseHandler<BodyStream> for BinaryStreamResponseHandler {
    fn handle(
        &self,
        _context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<BodyStream>, ProviderError>> {
        Box::pin(async move {
            Ok(Handled {
                value: response.body,
                raw: None,
                headers: response.headers,
            })
        })
    }
}

/// Builds a [`BinaryStreamResponseHandler`].
#[must_use]
pub fn binary_stream_response_handler() -> BinaryStreamResponseHandler {
    BinaryStreamResponseHandler
}

pub(super) fn stream_error(
    context: &ResponseContext,
    head: &ResponseHead,
    error: TransportError,
) -> ProviderError {
    if error.is_cancelled() {
        return ProviderError::Cancelled;
    }
    let retryable = error.is_retryable();
    context
        .api_error(format!(
            "failed to process successful response: {}",
            error.message
        ))
        .with_status(head.status)
        .with_response(head.headers.clone(), None)
        .retryable(retryable)
        .with_cause(error)
        .into()
}

/// Decodes a server-sent-event body into JSON chunks of type `T`.
///
/// `data: [DONE]` events are skipped; parse failures and transport errors
/// become [`ParseResult::Err`] items so the stream keeps going.
pub struct EventSourceResponseHandler<T> {
    max_event_bytes: usize,
    _marker: PhantomData<fn() -> T>,
}

impl<T> std::fmt::Debug for EventSourceResponseHandler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSourceResponseHandler")
            .field("max_event_bytes", &self.max_event_bytes)
            .finish()
    }
}

impl<T> EventSourceResponseHandler<T> {
    /// Creates the handler with the default event size limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_event_bytes: sse::DEFAULT_MAX_EVENT_BYTES,
            _marker: PhantomData,
        }
    }

    /// Sets the maximum size of one event.
    #[must_use]
    pub fn with_max_event_bytes(mut self, max_event_bytes: usize) -> Self {
        self.max_event_bytes = max_event_bytes;
        self
    }
}

impl<T> Default for EventSourceResponseHandler<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DeserializeOwned + Send + 'static> ResponseHandler<BoxStream<'static, ParseResult<T>>>
    for EventSourceResponseHandler<T>
{
    fn handle(
        &self,
        context: ResponseContext,
        response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<BoxStream<'static, ParseResult<T>>>, ProviderError>>
    {
        let max_event_bytes = self.max_event_bytes;
        Box::pin(async move {
            if response.headers.get_str("content-length") == Some("0") {
                return Err(EmptyResponseBodyError::new().into());
            }
            let head = response.head();
            let events = sse::decode_stream(response.body, max_event_bytes);
            let stream = events.filter_map(move |item| {
                let context = context.clone();
                let head = head.clone();
                async move {
                    match item {
                        Ok(event) if event.data == "[DONE]" => None,
                        Ok(event) => Some(parse_json_chunk::<T>(&event.data)),
                        Err(SseStreamError::Transport(error)) => Some(ParseResult::Err {
                            error: stream_error(&context, &head, error),
                            raw: None,
                        }),
                        Err(SseStreamError::Decode(error)) => Some(ParseResult::Err {
                            error: context
                                .api_error(format!("invalid event stream: {error}"))
                                .with_status(head.status)
                                .with_response(head.headers.clone(), None)
                                .into(),
                            raw: None,
                        }),
                    }
                }
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

/// Builds an [`EventSourceResponseHandler`].
#[must_use]
pub fn event_source_response_handler<T>() -> EventSourceResponseHandler<T> {
    EventSourceResponseHandler::new()
}
