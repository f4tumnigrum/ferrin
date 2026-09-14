//! Results of language model calls and shared request/response metadata.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::content::Content;
use super::finish_reason::FinishReason;
use super::stream_part::StreamPart;
use super::usage::Usage;
use crate::dynamic::BoxStream;
use crate::json::JsonValue;
use crate::shared::Headers;
use crate::shared::ModelId;
use crate::shared::ProviderMetadata;
use crate::shared::Warning;

/// Metadata about the request sent to the provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestMetadata {
    /// The JSON request body, when the adapter sends JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl RequestMetadata {
    /// Creates request metadata with a body.
    #[must_use]
    pub fn with_body(body: JsonValue) -> Self {
        Self { body: Some(body) }
    }
}

/// Metadata about the provider response.
///
/// Every field is optional because it may be unknown at the point where the
/// metadata is created (for example headers-only metadata at stream start).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseMetadata {
    /// Provider response id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Response timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    /// Model that produced the response (may differ from the requested id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<ModelId>,
    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
    /// Raw response body (non-streaming calls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl ResponseMetadata {
    /// Creates metadata that only carries headers.
    #[must_use]
    pub fn with_headers(headers: Headers) -> Self {
        Self {
            headers: Some(headers),
            ..Self::default()
        }
    }

    /// Creates metadata with timestamp and model id, as required by
    /// non-text modalities.
    #[must_use]
    pub fn new(timestamp: DateTime<Utc>, model_id: impl Into<ModelId>) -> Self {
        Self {
            timestamp: Some(timestamp),
            model_id: Some(model_id.into()),
            ..Self::default()
        }
    }

    /// Overlays fields that are `Some` in `other` onto `self`.
    pub fn merge(&mut self, other: ResponseMetadata) {
        if other.id.is_some() {
            self.id = other.id;
        }
        if other.timestamp.is_some() {
            self.timestamp = other.timestamp;
        }
        if other.model_id.is_some() {
            self.model_id = other.model_id;
        }
        if other.headers.is_some() {
            self.headers = other.headers;
        }
        if other.body.is_some() {
            self.body = other.body;
        }
    }
}

/// Result of `do_generate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateResult {
    /// Generated content parts in order.
    pub content: Vec<Content>,
    /// Why generation stopped.
    pub finish_reason: FinishReason,
    /// Token usage.
    #[serde(default)]
    pub usage: Usage,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Request metadata.
    #[serde(default)]
    pub request: RequestMetadata,
    /// Response metadata.
    #[serde(default)]
    pub response: ResponseMetadata,
    /// Warnings produced while preparing or executing the call.
    #[serde(default)]
    pub warnings: Vec<Warning>,
}

impl GenerateResult {
    /// Creates a result with content and finish reason; other fields default.
    #[must_use]
    pub fn new(content: Vec<Content>, finish_reason: FinishReason) -> Self {
        Self {
            content,
            finish_reason,
            usage: Usage::default(),
            provider_metadata: None,
            request: RequestMetadata::default(),
            response: ResponseMetadata::default(),
            warnings: Vec::new(),
        }
    }

    /// Concatenates all [`Content::Text`] parts.
    #[must_use]
    pub fn text(&self) -> String {
        self.content.iter().filter_map(Content::as_text).collect()
    }
}

/// Result of `do_stream`.
pub struct StreamResult {
    /// The stream of parts.
    pub stream: BoxStream<'static, StreamPart>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata known at stream start (typically headers only).
    pub response: ResponseMetadata,
}

impl StreamResult {
    /// Creates a stream result with default request and response metadata.
    #[must_use]
    pub fn new(stream: BoxStream<'static, StreamPart>) -> Self {
        Self {
            stream,
            request: RequestMetadata::default(),
            response: ResponseMetadata::default(),
        }
    }
}

impl std::fmt::Debug for StreamResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamResult")
            .field("stream", &"<stream>")
            .field("request", &self.request)
            .field("response", &self.response)
            .finish()
    }
}
