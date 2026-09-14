//! Ferrin provider utilities.
//!
//! Shared infrastructure for provider adapters:
//!
//! - [`http`]: the [`HttpTransport`] trait, request/response types, the
//!   reqwest-based default transport (feature `reqwest`), response handlers
//!   and the `post_json`/`get`/`delete` request helpers.
//! - [`sse`]: WHATWG server-sent-events decoding.
//! - [`secure_url`]: URL policy validation, DNS pinning and size-limited
//!   downloads.
//! - [`settings`]: credential and setting loading (parameter first, then
//!   environment variable).
//! - [`ids`]: id generation.
//! - [`stream_driver`]: the [`StreamMachine`](stream_driver::StreamMachine)
//!   trait, [`drive_stream`](stream_driver::drive_stream) and the
//!   early-error check shared by streaming language models.
//! - [`media_type`], [`reasoning`], [`tool_name_mapping`],
//!   [`streaming_tool_call`], [`retry`], [`provider_options`] and small
//!   helpers used across adapters.
//!
//! Design: `docs/01-architecture/14-http-and-security.md`, ADR 0009.

pub mod base_url;
pub mod batch;
pub mod headers;
pub mod http;
pub mod ids;
pub mod media_type;
pub mod provider_options;
pub mod provider_reference;
pub mod reasoning;
pub mod response_metadata;
pub mod retry;
pub mod secure_url;
pub mod settings;
pub mod sse;
pub mod stream_driver;
pub mod streaming_tool_call;
pub mod tool_name_mapping;

pub use http::ApiResponse;
pub use http::HttpRequest;
pub use http::HttpResponse;
pub use http::HttpTransport;
pub use http::MultipartForm;
pub use http::ParseResult;
pub use http::RequestBody;
pub use http::ResponseHandler;
pub use http::ResponseHandlers;
pub use http::SharedTransport;
pub use http::TransportError;
pub use ids::IdGenerator;
pub use ids::PrefixedIdGenerator;
pub use secure_url::UrlPolicy;
pub use settings::load_api_key;
pub use settings::load_optional_setting;
pub use settings::load_setting;
pub use sse::SseDecoder;
pub use sse::SseEvent;

#[cfg(feature = "reqwest")]
pub use http::ReqwestTransport;
#[cfg(feature = "reqwest")]
pub use http::default_transport;
/// The platform certificate verifier used by the default transport, re-exported
/// so applications can run its platform initialisation hooks.
#[cfg(feature = "platform-verifier")]
pub use rustls_platform_verifier;

/// Version of this crate, used in `User-Agent` suffixes.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
