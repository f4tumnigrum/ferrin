//! Model lookup, capability and reference errors.

use serde::Deserialize;
use serde::Serialize;

use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::shared::ProviderReference;

/// Kinds of models a provider can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ModelKind {
    /// Text generation.
    Language,
    /// Embeddings.
    Embedding,
    /// Image generation.
    Image,
    /// Speech-to-text.
    Transcription,
    /// Text-to-speech.
    Speech,
    /// Reranking.
    Reranking,
    /// Video generation.
    Video,
    /// Speech translation.
    SpeechTranslation,
    /// Realtime sessions.
    Realtime,
}

impl ModelKind {
    /// Returns a human-readable name (`language model`, `image model`, ...).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language model",
            Self::Embedding => "embedding model",
            Self::Image => "image model",
            Self::Transcription => "transcription model",
            Self::Speech => "speech model",
            Self::Reranking => "reranking model",
            Self::Video => "video model",
            Self::SpeechTranslation => "speech translation model",
            Self::Realtime => "realtime model",
        }
    }
}

impl std::fmt::Display for ModelKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The requested model does not exist or the provider has no models of that kind.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct NoSuchModelError {
    /// Provider that was asked, if known.
    pub provider: Option<ProviderId>,
    /// Requested model id.
    pub model_id: String,
    /// Requested model kind.
    pub model_kind: ModelKind,
    /// Explanation.
    pub message: String,
}

impl NoSuchModelError {
    /// Creates an error for an unknown `model_id` of `model_kind`.
    #[must_use]
    pub fn new(model_id: impl Into<String>, model_kind: ModelKind) -> Self {
        let model_id = model_id.into();
        Self {
            provider: None,
            message: format!("no such {model_kind}: {model_id}"),
            model_id,
            model_kind,
        }
    }

    /// Attaches the provider that was asked.
    #[must_use]
    pub fn with_provider(mut self, provider: &ProviderId) -> Self {
        self.provider = Some(provider.clone());
        self.message = format!(
            "no such {}: {} (provider {provider})",
            self.model_kind, self.model_id
        );
        self
    }

    /// Creates an error stating that `provider` exposes no models of `model_kind`.
    #[must_use]
    pub fn unsupported_kind(provider: &ProviderId, model_id: &str, model_kind: ModelKind) -> Self {
        Self {
            provider: Some(provider.clone()),
            model_id: model_id.to_owned(),
            model_kind,
            message: format!(
                "provider {provider} does not expose {model_kind}s (requested {model_id})"
            ),
        }
    }

    /// Overrides the message.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

/// A provider reference has no entry for the provider that received it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct NoSuchProviderReferenceError {
    /// Provider key that was looked up.
    pub provider: String,
    /// The reference that was received.
    pub reference: ProviderReference,
    /// Explanation.
    pub message: String,
}

impl NoSuchProviderReferenceError {
    /// Creates an error for `provider` missing in `reference`.
    #[must_use]
    pub fn new(provider: impl Into<String>, reference: ProviderReference) -> Self {
        let provider = provider.into();
        let available: Vec<&str> = reference.keys().map(String::as_str).collect();
        Self {
            message: format!(
                "no provider reference found for provider `{provider}`; available providers: {}",
                available.join(", ")
            ),
            provider,
            reference,
        }
    }
}

/// Too many values were passed to a single embedding call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "too many values for a single embedding call: {provider} model `{model_id}` accepts up to \
     {max_embeddings_per_call} values per call, got {value_count}"
)]
pub struct TooManyEmbeddingValuesForCallError {
    /// Provider identifier.
    pub provider: ProviderId,
    /// Model identifier.
    pub model_id: ModelId,
    /// Maximum values per call.
    pub max_embeddings_per_call: usize,
    /// Number of values that were passed.
    pub value_count: usize,
}

/// The provider or model does not support the requested functionality.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct UnsupportedFunctionalityError {
    /// Name of the functionality.
    pub functionality: String,
    /// Explanation.
    pub message: String,
}

impl UnsupportedFunctionalityError {
    /// Creates an error with the default message.
    #[must_use]
    pub fn new(functionality: impl Into<String>) -> Self {
        let functionality = functionality.into();
        Self {
            message: format!("`{functionality}` functionality not supported"),
            functionality,
        }
    }

    /// Creates an error with a custom message.
    #[must_use]
    pub fn with_message(functionality: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            functionality: functionality.into(),
            message: message.into(),
        }
    }
}
