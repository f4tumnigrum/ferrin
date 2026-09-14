//! Speech model (`<name>.speech`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::binary_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::speech_model::SpeechModel;
use ferrin_spec::speech_model::SpeechOptions;
use ferrin_spec::speech_model::SpeechResult;
use serde::Deserialize;
use serde::Serialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;

/// Output formats accepted by the API.
pub const OUTPUT_FORMATS: &[&str] = &["mp3", "opus", "aac", "flac", "wav", "pcm"];

/// Default voice.
pub const DEFAULT_VOICE: &str = "alloy";

/// Provider options of the speech model.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechProviderOptions {
    /// Voice instructions (gpt-4o-mini-tts).
    #[serde(default)]
    pub instructions: Option<String>,
    /// Playback speed (0.25 to 4.0).
    #[serde(default)]
    pub speed: Option<f64>,
}

#[derive(Debug, Serialize)]
struct SpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    response_format: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'a str>,
}

/// Media type of an output format.
#[must_use]
pub fn media_type_for_format(format: &str) -> MediaType {
    MediaType::new(match format {
        "mp3" => "audio/mpeg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "pcm" => "audio/pcm",
        _ => "application/octet-stream",
    })
}

/// Speech model backed by `POST /audio/speech`.
#[derive(Debug, Clone)]
pub struct OpenAiSpeechModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiSpeechModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("speech"),
            config,
            model_id: model_id.into(),
        }
    }
}

impl SpeechModel for OpenAiSpeechModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn do_generate(&self, options: SpeechOptions) -> Result<SpeechResult, ProviderError> {
        let mut warnings = Vec::new();
        let openai = parse_provider_options::<SpeechProviderOptions>(
            &self.config.provider_options_key,
            &options.provider_options,
        )?
        .unwrap_or_default();
        let format = match options.output_format.as_deref() {
            Some(format) if OUTPUT_FORMATS.contains(&format) => format.to_owned(),
            Some(format) => {
                warnings.push(Warning::unsupported_with_details(
                    "outputFormat",
                    format!("Unsupported output format: {format}. Using mp3 instead."),
                ));
                "mp3".to_owned()
            }
            None => "mp3".to_owned(),
        };
        if options.language.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "language",
                "OpenAI speech models do not support language selection. Language is determined by the voice used.",
            ));
        }
        let body = SpeechRequest {
            model: self.model_id.as_str(),
            input: &options.text,
            voice: options.voice.as_deref().unwrap_or(DEFAULT_VOICE),
            response_format: &format,
            speed: openai.speed.or(options.speed),
            instructions: openai
                .instructions
                .as_deref()
                .or(options.instructions.as_deref()),
        };
        let request_body = serde_json::to_value(&body).map_err(ProviderError::other)?;
        let handlers = ResponseHandlers::new(binary_response_handler(), failed_response_handler());
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("/audio/speech"),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(SpeechResult {
            audio: response.value,
            media_type: Some(media_type_for_format(&format)),
            warnings,
            request: RequestMetadata::with_body(request_body),
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: None,
            },
            provider_metadata: None,
        })
    }
}
