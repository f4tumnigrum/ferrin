//! Speech (text-to-speech) model interface.

use std::future::Future;

use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::language_model::RequestMetadata;
use crate::language_model::ResponseMetadata;
use crate::shared::Headers;
use crate::shared::MediaType;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::Warning;
use crate::shared::base64_bytes;

/// A model that synthesizes speech from text.
pub trait SpeechModel: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Model identifier.
    fn model_id(&self) -> &ModelId;

    /// Synthesizes `options.text`.
    fn do_generate(
        &self,
        options: SpeechOptions,
    ) -> impl Future<Output = Result<SpeechResult, ProviderError>> + Send;
}

/// Options for a speech synthesis call.
#[derive(Debug, Clone, Default)]
pub struct SpeechOptions {
    /// Text to synthesize.
    pub text: String,
    /// Voice identifier.
    pub voice: Option<String>,
    /// Output format (for example `mp3`, `wav`).
    pub output_format: Option<String>,
    /// Style instructions.
    pub instructions: Option<String>,
    /// Speed multiplier.
    pub speed: Option<f64>,
    /// Language code.
    pub language: Option<String>,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl SpeechOptions {
    /// Creates options for `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// Result of a speech synthesis call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechResult {
    /// Audio bytes.
    #[serde(with = "base64_bytes")]
    pub audio: Bytes,
    /// Media type of the audio, if the provider reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
    /// Request metadata.
    #[serde(default)]
    pub request: RequestMetadata,
    /// Response metadata; `timestamp` and `model_id` are expected to be set.
    #[serde(default)]
    pub response: ResponseMetadata,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}
