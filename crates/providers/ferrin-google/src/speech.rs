//! Gemini text-to-speech model (`generateContent` with `AUDIO` modality).

use base64::Engine;
use bytes::Bytes;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::shared::Warning;
use ferrin_spec::speech_model::SpeechModel;
use ferrin_spec::speech_model::SpeechOptions;
use ferrin_spec::speech_model::SpeechResult;
use serde::Deserialize;
use serde_json::json;

use crate::api_types::GenerateContentResponse;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::options::parse_merged;
use crate::output::OutputMapper;

/// Provider id family.
pub const FAMILY: &str = "speech";

/// Voice used when none is requested.
pub const DEFAULT_VOICE: &str = "Kore";

/// Sample rate assumed when the response media type carries none.
pub const DEFAULT_SAMPLE_RATE: u32 = 24_000;

/// Speech options (`provider_options["google"]`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoogleSpeechOptions {
    /// Multi-speaker voice configuration
    /// (`{speakerVoiceConfigs: [{speaker, voiceConfig: {prebuiltVoiceConfig: {voiceName}}}]}`).
    #[serde(default)]
    pub multi_speaker_voice_config: Option<JsonObject>,
}

/// Wraps signed 16-bit little-endian mono PCM in a 44-byte WAV header.
#[must_use]
pub fn add_wav_header(pcm: &[u8], sample_rate: u32) -> Bytes {
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let block_align = channels * bits_per_sample / 8;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32.saturating_add(data_size)).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(pcm);
    Bytes::from(out)
}

/// Sample rate encoded in a media type such as `audio/L16;codec=pcm;rate=24000`.
#[must_use]
pub fn parse_sample_rate(media_type: &str) -> Option<u32> {
    media_type
        .split(';')
        .map(str::trim)
        .find_map(|parameter| parameter.strip_prefix("rate="))
        .and_then(|rate| rate.parse().ok())
}

/// Text-to-speech model backed by the Gemini TTS models.
#[derive(Debug, Clone)]
pub struct GoogleSpeechModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

/// A prepared speech request.
#[derive(Debug, Clone)]
pub struct PreparedSpeechRequest {
    /// Request body.
    pub body: JsonValue,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// Whether raw PCM is returned instead of WAV.
    pub raw_pcm: bool,
}

impl GoogleSpeechModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id(FAMILY),
            config,
            model_id: model_id.into(),
        }
    }

    /// Builds the request body for `options`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidArgument`] for invalid provider options.
    pub fn prepare_request(
        &self,
        options: &SpeechOptions,
    ) -> Result<PreparedSpeechRequest, ProviderError> {
        let google = parse_merged::<GoogleSpeechOptions>(
            &self.config,
            &options.provider_options,
            |canonical, custom| {
                if custom.multi_speaker_voice_config.is_some() {
                    custom
                } else {
                    canonical
                }
            },
        )?;
        let mut warnings = Vec::new();
        let speech_config = match &google.multi_speaker_voice_config {
            Some(multi) => json!({"multiSpeakerVoiceConfig": multi}),
            None => json!({"voiceConfig": {"prebuiltVoiceConfig": {
                "voiceName": options.voice.as_deref().unwrap_or(DEFAULT_VOICE)
            }}}),
        };
        let mut prompt = options.text.clone();
        if let Some(instructions) = &options.instructions {
            if google.multi_speaker_voice_config.is_some() {
                warnings.push(Warning::unsupported_with_details(
                    "instructions",
                    "Google Gemini TTS ignores `instructions` when `multiSpeakerVoiceConfig` is set, because prepending them would break multi-speaker transcript parsing.",
                ));
            } else {
                prompt = format!("{instructions}: {}", options.text);
            }
        }
        if options.speed.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "speed",
                "Google Gemini TTS models do not support the `speed` option. It was ignored.",
            ));
        }
        if options.language.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "language",
                "Google Gemini TTS models do not support the `language` option. Language is detected automatically from the input text.",
            ));
        }
        let raw_pcm = match options.output_format.as_deref() {
            Some("pcm") => true,
            None | Some("wav") => false,
            Some(other) => {
                warnings.push(Warning::unsupported_with_details(
                    "outputFormat",
                    format!("Unsupported output format: {other}. Using wav instead."),
                ));
                false
            }
        };
        let body = json!({
            "contents": [{"role": "user", "parts": [{"text": prompt}]}],
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": speech_config,
            },
        });
        Ok(PreparedSpeechRequest {
            body,
            warnings,
            raw_pcm,
        })
    }
}

impl SpeechModel for GoogleSpeechModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: SpeechOptions) -> Result<SpeechResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let mut warnings = prepared.warnings;
        let handlers = ResponseHandlers::new(
            json_response_handler::<GenerateContentResponse>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config
                .model_url(self.model_id.as_str(), "generateContent"),
            self.config.headers(&options.headers)?,
            &prepared.body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let inline = response
            .value
            .candidates
            .iter()
            .flatten()
            .flat_map(|candidate| candidate.parts().iter())
            .find_map(|part| {
                part.inline_data
                    .as_ref()
                    .filter(|data| !data.data.is_empty())
            });
        let mime_type = inline.map(|data| data.mime_type.clone());
        let sample_rate = mime_type
            .as_deref()
            .and_then(parse_sample_rate)
            .unwrap_or(DEFAULT_SAMPLE_RATE);
        let pcm = match inline {
            Some(data) => base64::engine::general_purpose::STANDARD
                .decode(&data.data)
                .map_err(|error| {
                    ProviderError::InvalidResponseData(Box::new(
                        ferrin_spec::error::InvalidResponseDataError::new(
                            format!("invalid base64 audio data: {error}"),
                            JsonValue::Null,
                        ),
                    ))
                })?,
            None => Vec::new(),
        };
        let (audio, media_type) = if prepared.raw_pcm || pcm.is_empty() {
            if prepared.raw_pcm && !pcm.is_empty() {
                warnings.push(Warning::unsupported_with_details(
                    "outputFormat",
                    format!(
                        "Returning raw PCM audio (signed 16-bit little-endian, mono, {sample_rate} Hz). These bytes have no container header and are not directly playable; see providerMetadata.google for the sample rate and mime type."
                    ),
                ));
            }
            let media_type = mime_type.clone().map(MediaType::new);
            (Bytes::from(pcm), media_type)
        } else {
            (
                add_wav_header(&pcm, sample_rate),
                Some(MediaType::new("audio/wav")),
            )
        };
        let mapper = OutputMapper::new(self.config.clone(), Default::default());
        let mut metadata = JsonObject::new();
        metadata.insert("sampleRate".to_owned(), JsonValue::from(sample_rate));
        metadata.insert(
            "mimeType".to_owned(),
            mime_type.map_or(JsonValue::Null, JsonValue::from),
        );
        Ok(SpeechResult {
            audio,
            media_type,
            warnings,
            request: RequestMetadata::with_body(prepared.body),
            response: ResponseMetadata {
                id: response.value.response_id.clone(),
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            provider_metadata: Some(mapper.metadata(metadata)),
        })
    }
}
