//! Request helpers: send a request through a transport and run the response
//! handlers.

use bytes::Bytes;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use http::Method;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::handlers::ResponseContext;
use super::handlers::ResponseHandlers;
use super::transport::HttpRequest;
use super::transport::HttpTransport;
use super::transport::MultipartForm;
use super::transport::RequestBody;
use super::transport::TransportError;

/// A successful, handled response.
#[derive(Debug)]
pub struct ApiResponse<T> {
    /// The handler's value.
    pub value: T,
    /// Response headers.
    pub response_headers: Headers,
    /// Raw JSON body when the handler parsed JSON.
    pub raw: Option<JsonValue>,
}

/// Sends a JSON body with `POST`.
///
/// Adds `Content-Type: application/json` unless the caller set one.
///
/// # Errors
///
/// Returns the failure handler's error for non-2xx responses, an
/// `ApiCallError` for transport failures and [`ProviderError::Cancelled`]
/// when the token fires.
pub async fn post_json<T, B: Serialize + ?Sized>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    body: &B,
    handlers: &ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError> {
    let record = serde_json::to_value(body)
        .map_err(|error| invalid_request(&url, "failed to serialize request body", error))?;
    let bytes = serde_json::to_vec(&record)
        .map_err(|error| invalid_request(&url, "failed to serialize request body", error))?;
    let request = HttpRequest::new(Method::POST, url)
        .with_headers(headers)
        .with_body(RequestBody::json(Bytes::from(bytes)))
        .with_cancellation(cancellation);
    send(transport, request, Some(record), handlers).await
}

/// Sends a multipart form with `POST`.
///
/// # Errors
///
/// See [`post_json`].
pub async fn post_form<T>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    form: MultipartForm,
    handlers: &ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError> {
    let record = form.values();
    let request = HttpRequest::new(Method::POST, url)
        .with_headers(headers)
        .with_body(RequestBody::Multipart(form))
        .with_cancellation(cancellation);
    send(transport, request, Some(record), handlers).await
}

/// Sends raw bytes with `POST`.
///
/// # Errors
///
/// See [`post_json`].
pub async fn post_bytes<T>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    content_type: &str,
    data: Bytes,
    handlers: &ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError> {
    let request = HttpRequest::new(Method::POST, url)
        .with_headers(headers)
        .with_body(RequestBody::Bytes {
            content_type: content_type.to_owned(),
            data,
        })
        .with_cancellation(cancellation);
    send(transport, request, None, handlers).await
}

/// Sends a `GET` request.
///
/// # Errors
///
/// See [`post_json`].
pub async fn get<T>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    handlers: &ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError> {
    let request = HttpRequest::new(Method::GET, url)
        .with_headers(headers)
        .with_cancellation(cancellation);
    send(transport, request, None, handlers).await
}

/// Sends a `DELETE` request.
///
/// # Errors
///
/// See [`post_json`].
pub async fn delete<T>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    handlers: &ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError> {
    let request = HttpRequest::new(Method::DELETE, url)
        .with_headers(headers)
        .with_cancellation(cancellation);
    send(transport, request, None, handlers).await
}

/// Sends an arbitrary request and runs the handlers.
///
/// `request_body` is the JSON rendering of the body used in error reports.
/// A `Content-Type` header is added from the body when missing.
///
/// # Errors
///
/// See [`post_json`].
pub async fn send<T>(
    transport: &dyn HttpTransport,
    mut request: HttpRequest,
    request_body: Option<JsonValue>,
    handlers: &ResponseHandlers<T>,
) -> Result<ApiResponse<T>, ProviderError> {
    if !request.headers.contains("content-type")
        && let Some(content_type) = request.body.content_type()
    {
        request
            .headers
            .insert("content-type", &content_type)
            .map_err(|error| invalid_request(&request.url, "invalid content type header", error))?;
    }
    let context = ResponseContext::new(request.url.clone(), request_body);
    let response = transport
        .execute(request)
        .await
        .map_err(|error| transport_error(&context, error))?;

    if !response.status.is_success() {
        let status = response.status;
        let headers = response.headers.clone();
        let handled = handlers
            .failure
            .handle(context.clone(), response)
            .await
            .map_err(|error| {
                wrap_handler_error(
                    &context,
                    "failed to process error response",
                    status,
                    &headers,
                    error,
                )
            })?;
        return Err(handled.value);
    }

    let status = response.status;
    let headers = response.headers.clone();
    let handled = handlers
        .success
        .handle(context.clone(), response)
        .await
        .map_err(|error| {
            wrap_handler_error(
                &context,
                "failed to process successful response",
                status,
                &headers,
                error,
            )
        })?;
    Ok(ApiResponse {
        value: handled.value,
        response_headers: handled.headers,
        raw: handled.raw,
    })
}

fn invalid_request(
    url: &Url,
    message: &str,
    error: impl std::error::Error + Send + Sync + 'static,
) -> ProviderError {
    ApiCallError::new(message, url.clone())
        .retryable(false)
        .with_cause(error)
        .into()
}

fn transport_error(context: &ResponseContext, error: TransportError) -> ProviderError {
    if error.is_cancelled() {
        return ProviderError::Cancelled;
    }
    let retryable = error.is_retryable();
    context
        .api_error(format!("cannot connect to API: {}", error.message))
        .retryable(retryable)
        .with_cause(error)
        .into()
}

fn wrap_handler_error(
    context: &ResponseContext,
    message: &str,
    status: http::StatusCode,
    headers: &Headers,
    error: ProviderError,
) -> ProviderError {
    match error {
        ProviderError::ApiCall(_) | ProviderError::Cancelled => error,
        other => context
            .api_error(message)
            .with_status(status)
            .with_response(headers.clone(), None)
            .with_cause(other)
            .into(),
    }
}
