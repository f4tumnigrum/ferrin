//! HTTP transport abstraction and request helpers.
//!
//! Adapters never touch an HTTP client directly: they build an
//! [`HttpRequest`], hand it to an [`HttpTransport`] and interpret the
//! [`HttpResponse`] through [`ResponseHandler`]s. The request helpers in this
//! module ([`post_json`], [`get`], ...) tie those pieces together and map
//! transport failures to `ApiCallError`.

mod body;
mod handlers;
mod json_lines;
mod multipart;
mod request;
mod request_body;
#[cfg(feature = "reqwest")]
mod reqwest_transport;
mod transport;

pub use body::DEFAULT_MAX_RESPONSE_BYTES;
pub use body::read_body;
pub use handlers::BinaryResponseHandler;
pub use handlers::BinaryStreamResponseHandler;
pub use handlers::EventSourceResponseHandler;
pub use handlers::Handled;
pub use handlers::JsonErrorResponseHandler;

pub use handlers::JsonResponseHandler;
pub use handlers::ParseResult;
pub use handlers::ResponseContext;
pub use handlers::ResponseHandler;
pub use handlers::ResponseHandlers;
pub use handlers::StatusCodeErrorResponseHandler;
pub use handlers::TextResponseHandler;
pub use handlers::binary_response_handler;
pub use handlers::binary_stream_response_handler;
pub use handlers::event_source_response_handler;
pub use handlers::json_error_response_handler;

pub use handlers::json_response_handler;
pub use handlers::parse_json_chunk;
pub use handlers::status_code_error_response_handler;
pub use handlers::text_response_handler;
pub use json_lines::JsonLinesResponseHandler;
pub use json_lines::json_lines_response_handler;
pub use multipart::MultipartForm;
pub use multipart::MultipartPart;
pub use request::ApiResponse;
pub use request::delete;
pub use request::get;
pub use request::post_bytes;
pub use request::post_form;
pub use request::post_json;
pub use request::send;
pub use request_body::RequestBody;
#[cfg(feature = "reqwest")]
pub use reqwest_transport::ReqwestTransport;
#[cfg(feature = "reqwest")]
pub use reqwest_transport::default_transport;
pub use transport::BodyStream;
pub use transport::HttpRequest;
pub use transport::HttpResponse;
pub use transport::HttpTransport;
pub use transport::ResponseHead;
pub use transport::SharedTransport;
pub use transport::TransportError;
pub use transport::TransportErrorKind;
