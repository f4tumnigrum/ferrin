//! Video model interface.
//!
//! A video model implements the synchronous [`VideoModel::do_generate`], the
//! asynchronous operation pair [`VideoModel::do_start`] /
//! [`VideoModel::do_status`], or both. Capability queries
//! (`supports_generate`, `supports_operations`, `supports_webhook`) let the
//! core choose a flow before calling. Polling, timeouts and webhook waiting
//! are implemented by the core, not by adapters.

use std::future::Future;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::dynamic::BoxFuture;
use crate::error::ProviderError;
use crate::image_model::AspectRatio;
use crate::image_model::ImageSize;
use crate::json::JsonValue;
use crate::language_model::ResponseMetadata;
use crate::shared::FileData;
use crate::shared::Headers;
use crate::shared::MediaType;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;

/// A model that generates videos.
pub trait VideoModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Maximum number of videos per call, or `None` when unknown (treated as 1).
    fn max_videos_per_call(&self) -> Option<usize>;

    /// Whether [`do_generate`](Self::do_generate) is implemented.
    fn supports_generate(&self) -> bool {
        false
    }

    /// Generates videos and waits for the result in one call.
    fn do_generate(
        &self,
        options: VideoOptions,
    ) -> impl Future<Output = Result<VideoResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported(
            "synchronous video generation",
        )))
    }

    /// Whether [`do_start`](Self::do_start) and [`do_status`](Self::do_status)
    /// are implemented.
    fn supports_operations(&self) -> bool {
        false
    }

    /// Starts an asynchronous generation operation.
    fn do_start(
        &self,
        options: VideoStartOptions,
    ) -> impl Future<Output = Result<VideoStartResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported(
            "asynchronous video generation",
        )))
    }

    /// Queries the status of an operation returned by `do_start`.
    fn do_status(
        &self,
        options: VideoStatusOptions,
    ) -> impl Future<Output = Result<VideoStatusResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported(
            "asynchronous video generation",
        )))
    }

    /// Whether the provider can deliver completion through a webhook.
    fn supports_webhook(&self) -> bool {
        false
    }

    /// Prepares a webhook for an operation.
    ///
    /// The default implementation invokes `factory` unchanged. Providers that
    /// need to register the URL or wrap the payload override this method.
    fn handle_webhook(
        &self,
        factory: WebhookFactory,
    ) -> impl Future<Output = Result<WebhookHandle, ProviderError>> + Send {
        factory()
    }
}

/// Aspect ratio for video: a fixed ratio or provider-chosen (`adaptive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VideoAspectRatio {
    /// A fixed ratio such as `16:9`.
    Ratio(AspectRatio),
    /// Let the provider choose based on the input.
    #[serde(with = "adaptive")]
    Adaptive,
}

mod adaptive {
    pub(super) fn serialize<S: serde::Serializer>(serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("adaptive")
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<(), D::Error> {
        let text = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(deserializer)?;
        if text == "adaptive" {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected `adaptive`"))
        }
    }
}

/// Which frame an input image anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FrameType {
    /// The first frame.
    FirstFrame,
    /// The last frame.
    LastFrame,
}

/// An input file for video generation: inline bytes or a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoFile {
    /// File payload.
    pub data: FileData,
    /// Media type, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// Provider-specific options for this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// An input image anchored to a frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameImage {
    /// The image.
    pub image: VideoFile,
    /// Which frame it anchors.
    pub frame_type: FrameType,
}

/// Options for a video generation call.
#[derive(Debug, Clone)]
pub struct VideoOptions {
    /// Text prompt.
    pub prompt: Option<String>,
    /// Number of videos.
    pub n: u32,
    /// Aspect ratio.
    pub aspect_ratio: Option<VideoAspectRatio>,
    /// Resolution in pixels.
    pub resolution: Option<ImageSize>,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// Frames per second.
    pub fps: Option<u32>,
    /// Random seed.
    pub seed: Option<u64>,
    /// Single input image.
    pub image: Option<VideoFile>,
    /// Frame-anchored input images.
    pub frame_images: Vec<FrameImage>,
    /// Additional reference inputs.
    pub input_references: Vec<VideoFile>,
    /// Whether to generate audio.
    pub generate_audio: Option<bool>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl VideoOptions {
    /// Creates options for `prompt` requesting one video.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: Some(prompt.into()),
            ..Self::default()
        }
    }
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            prompt: None,
            n: 1,
            aspect_ratio: None,
            resolution: None,
            duration: None,
            fps: None,
            seed: None,
            image: None,
            frame_images: Vec::new(),
            input_references: Vec::new(),
            generate_audio: None,
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Options for starting an asynchronous operation.
#[derive(Debug, Clone)]
pub struct VideoStartOptions {
    /// Generation options.
    pub options: VideoOptions,
    /// Webhook URL the provider should call on completion.
    pub webhook_url: Option<Url>,
}

/// Options for querying an operation.
#[derive(Debug, Clone)]
pub struct VideoStatusOptions {
    /// Opaque operation handle returned by `do_start`.
    pub operation: JsonValue,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

/// A generated video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoData {
    /// Payload: inline bytes or a URL.
    pub data: FileData,
    /// Media type.
    pub media_type: MediaType,
}

/// Result of a synchronous generation or a completed operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoResult {
    /// Generated videos.
    pub videos: Vec<VideoData>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Response metadata; `timestamp` and `model_id` are expected to be set.
    #[serde(default)]
    pub response: ResponseMetadata,
}

/// Result of starting an operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoStartResult {
    /// Opaque operation handle to pass to `do_status`.
    pub operation: JsonValue,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Response metadata.
    #[serde(default)]
    pub response: ResponseMetadata,
}

/// Status of an operation, tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
#[non_exhaustive]
pub enum VideoStatusResult {
    /// Still running.
    Pending {
        /// Warnings.
        #[serde(default)]
        warnings: Vec<Warning>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
        /// Response metadata.
        #[serde(default)]
        response: ResponseMetadata,
    },
    /// Finished successfully.
    Completed {
        /// Generated videos.
        videos: Vec<VideoData>,
        /// Warnings.
        #[serde(default)]
        warnings: Vec<Warning>,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
        /// Response metadata.
        #[serde(default)]
        response: ResponseMetadata,
    },
    /// Failed.
    Error {
        /// Error message reported by the provider.
        error: String,
        /// Provider-specific metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
        /// Response metadata.
        #[serde(default)]
        response: ResponseMetadata,
    },
}

/// Payload delivered to a webhook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookPayload {
    /// Request headers of the callback.
    pub headers: Headers,
    /// Request body of the callback.
    pub body: JsonValue,
}

/// A prepared webhook: the URL to hand to the provider and a future that
/// resolves when the callback arrives.
pub struct WebhookHandle {
    /// Publicly reachable callback URL.
    pub url: Url,
    /// Resolves with the callback payload.
    pub received: BoxFuture<'static, Result<WebhookPayload, ProviderError>>,
}

impl std::fmt::Debug for WebhookHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookHandle")
            .field("url", &self.url)
            .field("received", &"<future>")
            .finish()
    }
}

/// Creates webhooks; implemented by the application (the SDK runs no server).
pub type WebhookFactory =
    Arc<dyn Fn() -> BoxFuture<'static, Result<WebhookHandle, ProviderError>> + Send + Sync>;
