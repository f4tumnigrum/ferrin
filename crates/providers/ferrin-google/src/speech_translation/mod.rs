//! Google Live speech translation adapted from Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.); see the root NOTICE.

mod mapper;

use ferrin_spec::JsonObject;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::speech_translation_model::SpeechTranslationModel;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamResult;
use serde::Deserialize;
use serde_json::json;

use crate::config::GoogleConfig;
use crate::config::SharedConfig;
use crate::live_audio;
use crate::options::parse_merged;

/// Provider options for Live speech translation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSpeechTranslationOptions {
    /// Echo input already in the target language instead of producing silence.
    pub echo_target_language: Option<bool>,
}

impl GoogleSpeechTranslationOptions {
    fn merge(mut self, other: Self) -> Self {
        if other.echo_target_language.is_some() {
            self.echo_target_language = other.echo_target_language;
        }
        self
    }
}

/// Streaming speech translation over the Gemini Live API.
#[derive(Debug, Clone)]
pub struct GoogleSpeechTranslationModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl GoogleSpeechTranslationModel {
    /// Creates a Live speech translation model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("speech-translation"),
            config,
            model_id: model_id.into(),
        }
    }
}

impl SpeechTranslationModel for GoogleSpeechTranslationModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> Result<SpeechTranslationStreamResult, ProviderError> {
        live_audio::validate_format(&options.input_audio_format)?;
        if options.target_language.trim().is_empty() {
            return Err(InvalidArgumentError::new(
                "target_language",
                "target language must not be empty",
            )
            .into());
        }
        let google = parse_merged::<GoogleSpeechTranslationOptions>(
            &self.config,
            &options.provider_options,
            GoogleSpeechTranslationOptions::merge,
        )?;
        let mut translation = JsonObject::from_iter([(
            "targetLanguageCode".to_owned(),
            json!(options.target_language),
        )]);
        if let Some(echo) = google.echo_target_language {
            translation.insert("echoTargetLanguage".to_owned(), json!(echo));
        }
        let setup = json!({
            "model": GoogleConfig::model_path(self.model_id.as_str()),
            "generationConfig": {"responseModalities": ["AUDIO"], "translationConfig": translation},
            "inputAudioTranscription": {}, "outputAudioTranscription": {},
        });
        let mut warnings = Vec::new();
        if options.source_language.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "source_language",
                "Google Live automatically detects the source language",
            ));
        }
        if options.output_audio_format.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "output_audio_format",
                "Google Live returns signed 16-bit PCM at 24 kHz",
            ));
        }
        let request = RequestMetadata::with_body(setup.clone());
        let mapper = mapper::Translation::new(self.config.clone(), warnings);
        let (stream, response) = live_audio::start(
            &self.config,
            self.model_id.clone(),
            live_audio::Options {
                setup,
                audio: options.audio,
                headers: options.headers,
                cancellation: options.cancellation,
                include_raw: options.include_raw_chunks,
            },
            mapper,
        )
        .await?;
        Ok(SpeechTranslationStreamResult {
            stream,
            request,
            response,
        })
    }
}
