//! Image model interface.

use std::future::Future;
use std::str::FromStr;

use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::language_model::ResponseMetadata;
use crate::shared::FileData;
use crate::shared::Headers;
use crate::shared::MediaType;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;
use crate::shared::base64_bytes;

/// A model that generates images from a prompt.
pub trait ImageModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Maximum number of images per call, or `None` when unknown (treated as 1).
    fn max_images_per_call(&self) -> Option<usize>;

    /// Generates `options.n` images.
    fn do_generate(
        &self,
        options: ImageOptions,
    ) -> impl Future<Output = Result<ImageResult, ProviderError>> + Send;
}

/// Options for an image generation call.
#[derive(Debug, Clone)]
pub struct ImageOptions {
    /// Text prompt; `None` for pure edit/variation calls.
    pub prompt: Option<String>,
    /// Number of images to generate.
    pub n: u32,
    /// Requested size.
    pub size: Option<ImageSize>,
    /// Requested aspect ratio.
    pub aspect_ratio: Option<AspectRatio>,
    /// Random seed.
    pub seed: Option<u64>,
    /// Reference or input images.
    pub files: Vec<ImageFile>,
    /// Mask image for edits.
    pub mask: Option<ImageFile>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl ImageOptions {
    /// Creates options for `prompt` requesting one image.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: Some(prompt.into()),
            ..Self::default()
        }
    }
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            prompt: None,
            n: 1,
            size: None,
            aspect_ratio: None,
            seed: None,
            files: Vec::new(),
            mask: None,
            provider_options: ProviderOptions::new(),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// An input image: inline bytes or a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageFile {
    /// Image payload.
    pub data: FileData,
    /// Media type, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// Provider-specific options for this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// A generated image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedImage {
    /// Image bytes.
    #[serde(with = "base64_bytes")]
    pub data: Bytes,
    /// Media type, if the provider reports it; otherwise detected by the core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
}

/// Token usage of an image call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageUsage {
    /// Input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Total tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

/// Result of an image generation call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageResult {
    /// Generated images.
    pub images: Vec<GeneratedImage>,
    /// Whether an empty result may be retried; `None` means "use the default".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_retryable: Option<bool>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Provider-specific metadata (per-image metadata under `images`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Response metadata; `timestamp` and `model_id` are expected to be set.
    #[serde(default)]
    pub response: ResponseMetadata,
    /// Token usage, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ImageUsage>,
}

/// Image size in pixels, serialized as `WIDTHxHEIGHT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ImageSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl ImageSize {
    /// Creates a size.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl std::fmt::Display for ImageSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl FromStr for ImageSize {
    type Err = InvalidDimension;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_pair(text, 'x')
            .map(|(width, height)| Self { width, height })
            .ok_or_else(|| InvalidDimension {
                text: text.to_owned(),
                expected: "WIDTHxHEIGHT",
            })
    }
}

impl TryFrom<String> for ImageSize {
    type Error = InvalidDimension;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<ImageSize> for String {
    fn from(size: ImageSize) -> Self {
        size.to_string()
    }
}

/// Aspect ratio, serialized as `WIDTH:HEIGHT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AspectRatio {
    /// Horizontal component.
    pub width: u32,
    /// Vertical component.
    pub height: u32,
}

impl AspectRatio {
    /// Creates a ratio.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl std::fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.width, self.height)
    }
}

impl FromStr for AspectRatio {
    type Err = InvalidDimension;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_pair(text, ':')
            .map(|(width, height)| Self { width, height })
            .ok_or_else(|| InvalidDimension {
                text: text.to_owned(),
                expected: "WIDTH:HEIGHT",
            })
    }
}

impl TryFrom<String> for AspectRatio {
    type Error = InvalidDimension;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<AspectRatio> for String {
    fn from(ratio: AspectRatio) -> Self {
        ratio.to_string()
    }
}

fn parse_pair(text: &str, separator: char) -> Option<(u32, u32)> {
    let (left, right) = text.trim().split_once(separator)?;
    let left: u32 = left.trim().parse().ok()?;
    let right: u32 = right.trim().parse().ok()?;
    (left > 0 && right > 0).then_some((left, right))
}

/// Error returned when a size or aspect ratio string is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid dimension `{text}`: expected `{expected}`")]
pub struct InvalidDimension {
    /// The rejected text.
    pub text: String,
    /// The expected format.
    pub expected: &'static str,
}
