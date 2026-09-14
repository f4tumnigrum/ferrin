//! Speech synthesis: [`generate_speech`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §3.

use std::future::IntoFuture;

use bytes::Bytes;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_spec::BoxFuture;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::Warning;
use ferrin_spec::speech_model::SpeechOptions;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::retry;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Media type used when neither the provider nor detection knows better.
const DEFAULT_AUDIO_MEDIA_TYPE: &str = "audio/mpeg";

/// Generated audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAudio {
    /// Audio bytes.
    pub data: Bytes,
    /// Media type (reported, detected, or `audio/mpeg`).
    pub media_type: MediaType,
    /// Container format derived from the media type (`mp3` for
    /// `audio/mpeg`, otherwise the subtype).
    pub format: String,
}

impl GeneratedAudio {
    /// Builds the audio value, deriving `format` from the media type.
    #[must_use]
    pub fn new(data: Bytes, media_type: MediaType) -> Self {
        let format = match media_type.subtype() {
            Some(subtype) if media_type.as_str() != "audio/mpeg" && !subtype.is_empty() => {
                subtype.to_owned()
            }
            _ => "mp3".to_owned(),
        };
        Self {
            data,
            media_type,
            format,
        }
    }

    /// The audio bytes as base64.
    #[must_use]
    pub fn base64(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// Result of [`generate_speech`].
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateSpeechResult {
    /// The generated audio.
    pub audio: GeneratedAudio,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata of the calls made.
    pub responses: Vec<ResponseMetadata>,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Synthesizes speech from `text`.
#[must_use]
pub fn generate_speech(
    model: impl Into<SpeechModelRef>,
    text: impl Into<String>,
) -> GenerateSpeech {
    GenerateSpeech {
        model: model.into(),
        text: text.into(),
        voice: None,
        output_format: None,
        instructions: None,
        speed: None,
        language: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`generate_speech`]; `.await` runs the call.
#[derive(Debug)]
pub struct GenerateSpeech {
    model: SpeechModelRef,
    text: String,
    voice: Option<String>,
    output_format: Option<String>,
    instructions: Option<String>,
    speed: Option<f64>,
    language: Option<String>,
    base: ModalityOptions,
}

impl GenerateSpeech {
    /// Voice to use.
    #[must_use]
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    /// Output format (`mp3`, `wav`, ...).
    #[must_use]
    pub fn output_format(mut self, output_format: impl Into<String>) -> Self {
        self.output_format = Some(output_format.into());
        self
    }

    /// Style instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Speed multiplier.
    #[must_use]
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Language code.
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
}

impl_modality_builder!(GenerateSpeech);

impl IntoFuture for GenerateSpeech {
    type Output = Result<GenerateSpeechResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(self))
    }
}

async fn run(builder: GenerateSpeech) -> Result<GenerateSpeechResult, Error> {
    let model = resolve_model(&builder.model, ProviderRegistry::speech_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("speech", &identity);
    let base = builder.base.clone();
    base.run(|base, token| {
        async move {
            let headers = base.request_headers();
            let result = retry(&base.retry_policy, &token, |_| {
                let options = SpeechOptions {
                    text: builder.text.clone(),
                    voice: builder.voice.clone(),
                    output_format: builder.output_format.clone(),
                    instructions: builder.instructions.clone(),
                    speed: builder.speed,
                    language: builder.language.clone(),
                    provider_options: base.provider_options.clone(),
                    headers: headers.clone(),
                    cancellation: token.child_token(),
                };
                let model = &model;
                async move { model.do_generate(options).await.map_err(Error::from) }
            })
            .await?;
            if result.audio.is_empty() {
                return Err(Error::NoSpeechGenerated {
                    responses: vec![result.response],
                });
            }
            spans::log_warnings(&result.warnings, &identity);
            let media_type = result
                .media_type
                .clone()
                .or_else(|| detect_media_type_for(&result.audio, "audio"))
                .unwrap_or_else(|| MediaType::new(DEFAULT_AUDIO_MEDIA_TYPE));
            Ok(GenerateSpeechResult {
                audio: GeneratedAudio::new(result.audio, media_type),
                warnings: result.warnings,
                request: result.request,
                responses: vec![result.response],
                provider_metadata: result.provider_metadata,
            })
        }
        .instrument(span)
    })
    .await
}
