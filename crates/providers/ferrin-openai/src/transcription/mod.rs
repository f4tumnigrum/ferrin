//! Transcription model (`<name>.transcription`).

#[cfg(feature = "realtime")]
pub mod realtime_stream;

use chrono::Utc;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::media_type::media_type_to_extension;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionResult;
use ferrin_spec::transcription_model::TranscriptionSegment;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamResult;
use serde::Deserialize;
use serde_json::json;

use crate::config::SharedConfig;
use crate::embedding::provider_metadata;
use crate::error::failed_response_handler;

/// Model that only supports streaming over the realtime WebSocket.
const REALTIME_MODEL: &str = "gpt-realtime-whisper";
/// Model that produces speaker-labelled segments.
const DIARIZATION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// Whether `model_id` is a realtime transcription model.
#[must_use]
pub fn is_realtime_model(model_id: &str) -> bool {
    model_id == REALTIME_MODEL || model_id.starts_with("gpt-realtime-whisper-")
}

/// Language names returned by the API mapped to ISO-639-1 codes.
const LANGUAGE_CODES: &[(&str, &str)] = &[
    ("afrikaans", "af"),
    ("arabic", "ar"),
    ("armenian", "hy"),
    ("azerbaijani", "az"),
    ("belarusian", "be"),
    ("bosnian", "bs"),
    ("bulgarian", "bg"),
    ("catalan", "ca"),
    ("chinese", "zh"),
    ("croatian", "hr"),
    ("czech", "cs"),
    ("danish", "da"),
    ("dutch", "nl"),
    ("english", "en"),
    ("estonian", "et"),
    ("finnish", "fi"),
    ("french", "fr"),
    ("galician", "gl"),
    ("german", "de"),
    ("greek", "el"),
    ("hebrew", "he"),
    ("hindi", "hi"),
    ("hungarian", "hu"),
    ("icelandic", "is"),
    ("indonesian", "id"),
    ("italian", "it"),
    ("japanese", "ja"),
    ("kannada", "kn"),
    ("kazakh", "kk"),
    ("korean", "ko"),
    ("latvian", "lv"),
    ("lithuanian", "lt"),
    ("macedonian", "mk"),
    ("malay", "ms"),
    ("marathi", "mr"),
    ("maori", "mi"),
    ("nepali", "ne"),
    ("norwegian", "no"),
    ("persian", "fa"),
    ("polish", "pl"),
    ("portuguese", "pt"),
    ("romanian", "ro"),
    ("russian", "ru"),
    ("serbian", "sr"),
    ("slovak", "sk"),
    ("slovenian", "sl"),
    ("spanish", "es"),
    ("swahili", "sw"),
    ("swedish", "sv"),
    ("tagalog", "tl"),
    ("tamil", "ta"),
    ("thai", "th"),
    ("turkish", "tr"),
    ("ukrainian", "uk"),
    ("urdu", "ur"),
    ("vietnamese", "vi"),
    ("welsh", "cy"),
];

/// Maps a language name to its ISO-639-1 code.
#[must_use]
pub fn language_code(name: &str) -> Option<&'static str> {
    let lower = name.trim().to_ascii_lowercase();
    LANGUAGE_CODES
        .iter()
        .find(|(language, _)| *language == lower)
        .map(|(_, code)| *code)
}

/// Audio chunking strategy.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ChunkingStrategy {
    /// A named strategy (`auto`).
    Named(String),
    /// Server-side voice activity detection.
    #[serde(rename_all = "camelCase")]
    ServerVad {
        /// Strategy type (`server_vad`).
        #[serde(rename = "type")]
        kind: String,
        /// Activation threshold.
        #[serde(default)]
        threshold: Option<f64>,
        /// Audio kept before speech starts (ms).
        #[serde(default)]
        prefix_padding_ms: Option<u64>,
        /// Silence ending a chunk (ms).
        #[serde(default)]
        silence_duration_ms: Option<u64>,
    },
}

impl ChunkingStrategy {
    fn to_form_value(&self) -> String {
        match self {
            Self::Named(name) => name.clone(),
            Self::ServerVad {
                kind,
                threshold,
                prefix_padding_ms,
                silence_duration_ms,
            } => {
                let mut object = JsonObject::new();
                object.insert("type".to_owned(), JsonValue::from(kind.as_str()));
                if let Some(threshold) = threshold {
                    object.insert("threshold".to_owned(), json!(threshold));
                }
                if let Some(ms) = prefix_padding_ms {
                    object.insert("prefix_padding_ms".to_owned(), json!(ms));
                }
                if let Some(ms) = silence_duration_ms {
                    object.insert("silence_duration_ms".to_owned(), json!(ms));
                }
                JsonValue::Object(object).to_string()
            }
        }
    }
}

/// Options of the streaming (realtime) transcription.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamingOptions {
    /// Latency/accuracy trade-off (`minimal`, `low`, `medium`, `high`, `xhigh`).
    #[serde(default)]
    pub delay: Option<String>,
    /// Extra fields to include in realtime events.
    #[serde(default)]
    pub include: Option<Vec<String>>,
}

/// Call-level provider options.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptionProviderOptions {
    /// Extra response fields.
    #[serde(default)]
    pub include: Option<Vec<String>>,
    /// Input language (ISO-639-1).
    #[serde(default)]
    pub language: Option<String>,
    /// Style prompt.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Sampling temperature.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Timestamp granularities (`word`, `segment`).
    #[serde(default)]
    pub timestamp_granularities: Option<Vec<String>>,
    /// Response format (`json`, `verbose_json`, `diarized_json`).
    #[serde(default)]
    pub response_format: Option<String>,
    /// Chunking strategy.
    #[serde(default)]
    pub chunking_strategy: Option<ChunkingStrategy>,
    /// Streaming options.
    #[serde(default)]
    pub streaming: Option<StreamingOptions>,
}

/// A word with timestamps.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionWord {
    /// Word.
    pub word: String,
    /// Start (seconds).
    pub start: f64,
    /// End (seconds).
    pub end: f64,
}

/// A segment with timestamps.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionResponseSegment {
    /// Start (seconds).
    pub start: f64,
    /// End (seconds).
    pub end: f64,
    /// Text.
    pub text: String,
    /// Speaker label (diarization).
    #[serde(default)]
    pub speaker: Option<String>,
}

/// Response body.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionResponse {
    /// Transcript.
    pub text: String,
    /// Language name.
    #[serde(default)]
    pub language: Option<String>,
    /// Duration (seconds).
    #[serde(default)]
    pub duration: Option<f64>,
    /// Words.
    #[serde(default)]
    pub words: Option<Vec<TranscriptionWord>>,
    /// Segments.
    #[serde(default)]
    pub segments: Option<Vec<TranscriptionResponseSegment>>,
}

/// Transcription model.
#[derive(Debug, Clone)]
pub struct OpenAiTranscriptionModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiTranscriptionModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("transcription"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    pub(crate) fn provider_options(
        &self,
        options: &ferrin_spec::ProviderOptions,
    ) -> Result<Option<TranscriptionProviderOptions>, ProviderError> {
        let key = &self.config.provider_options_key;
        let parsed = parse_provider_options::<TranscriptionProviderOptions>(key, options)?;
        if parsed.is_none() && key != "openai" {
            return Ok(parse_provider_options("openai", options)?);
        }
        Ok(parsed)
    }

    /// Builds the multipart form.
    fn build_form(
        &self,
        options: &TranscriptionOptions,
        openai: Option<&TranscriptionProviderOptions>,
    ) -> MultipartForm {
        let model_id = self.model_id.as_str();
        let extension = media_type_to_extension(&options.media_type);
        let mut form = MultipartForm::new().field("model", model_id).file(
            "file",
            Some(format!("audio.{extension}")),
            Some(options.media_type.as_str().to_owned()),
            options.audio.clone(),
        );
        if model_id == "whisper-1" {
            form = form.field("response_format", "verbose_json");
        }
        let is_diarization = model_id == DIARIZATION_MODEL;
        let chunking = openai
            .and_then(|o| o.chunking_strategy.clone())
            .or_else(|| is_diarization.then(|| ChunkingStrategy::Named("auto".to_owned())));
        match openai {
            Some(openai) => {
                let is_gpt4o = matches!(model_id, "gpt-4o-transcribe" | "gpt-4o-mini-transcribe");
                for include in openai.include.iter().flatten() {
                    form = form.field("include[]", include);
                }
                if let Some(language) = &openai.language {
                    form = form.field("language", language);
                }
                if let Some(prompt) = &openai.prompt {
                    form = form.field("prompt", prompt);
                }
                if model_id != "whisper-1" {
                    let format = openai.response_format.clone().unwrap_or_else(|| {
                        if is_diarization {
                            "diarized_json".to_owned()
                        } else if is_gpt4o {
                            "json".to_owned()
                        } else {
                            "verbose_json".to_owned()
                        }
                    });
                    form = form.field("response_format", format);
                }
                if let Some(temperature) = openai.temperature {
                    form = form.field("temperature", temperature.to_string());
                }
                for granularity in openai.timestamp_granularities.iter().flatten() {
                    form = form.field("timestamp_granularities[]", granularity);
                }
            }
            None if is_diarization => {
                form = form.field("response_format", "diarized_json");
            }
            None => {}
        }
        if let Some(chunking) = chunking {
            form = form.field("chunking_strategy", chunking.to_form_value());
        }
        form
    }
}

impl TranscriptionModel for OpenAiTranscriptionModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> Result<TranscriptionResult, ProviderError> {
        if is_realtime_model(self.model_id.as_str()) {
            return Err(UnsupportedFunctionalityError::new(format!(
                "non-streaming transcription with {}",
                self.model_id
            ))
            .into());
        }
        let openai = self.provider_options(&options.provider_options)?;
        let form = self.build_form(&options, openai.as_ref());
        let request_body = form.values();
        let handlers = ResponseHandlers::new(
            json_response_handler::<TranscriptionResponse>(),
            failed_response_handler(),
        );
        let response = post_form(
            self.config.transport.as_ref(),
            self.config.url("/audio/transcriptions"),
            self.config.headers(&options.headers)?,
            form,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let body = response.value;
        let language = body
            .language
            .as_deref()
            .and_then(language_code)
            .map(str::to_owned);
        let segments: Vec<TranscriptionSegment> = match (&body.segments, &body.words) {
            (Some(segments), _) => segments
                .iter()
                .map(|segment| TranscriptionSegment {
                    text: segment.text.clone(),
                    start_second: segment.start,
                    end_second: segment.end,
                })
                .collect(),
            (None, Some(words)) => words
                .iter()
                .map(|word| TranscriptionSegment {
                    text: word.word.clone(),
                    start_second: word.start,
                    end_second: word.end,
                })
                .collect(),
            (None, None) => Vec::new(),
        };
        let diarized: Vec<JsonValue> = body
            .segments
            .iter()
            .flatten()
            .filter_map(|segment| {
                segment.speaker.as_ref().map(|speaker| {
                    json!({
                        "text": segment.text,
                        "startSecond": segment.start,
                        "endSecond": segment.end,
                        "speaker": speaker,
                    })
                })
            })
            .collect();
        let metadata = (!diarized.is_empty()).then(|| {
            let mut object = JsonObject::new();
            object.insert("segments".to_owned(), JsonValue::Array(diarized));
            provider_metadata(&self.config.provider_options_key, object)
        });
        Ok(TranscriptionResult {
            text: body.text,
            segments,
            language,
            duration_in_seconds: body.duration,
            warnings: Vec::new(),
            request: RequestMetadata::with_body(request_body),
            response: ResponseMetadata {
                id: None,
                timestamp: Some(Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            provider_metadata: metadata,
        })
    }

    fn supports_stream(&self) -> bool {
        cfg!(feature = "realtime") && is_realtime_model(self.model_id.as_str())
    }

    async fn do_stream(
        &self,
        options: TranscriptionStreamOptions,
    ) -> Result<TranscriptionStreamResult, ProviderError> {
        if !is_realtime_model(self.model_id.as_str()) {
            return Err(UnsupportedFunctionalityError::new(format!(
                "streaming transcription with {}",
                self.model_id
            ))
            .into());
        }
        #[cfg(feature = "realtime")]
        {
            realtime_stream::stream(self, options).await
        }
        #[cfg(not(feature = "realtime"))]
        {
            let _ = options;
            Err(UnsupportedFunctionalityError::with_message(
                "streaming transcription",
                "enable the `realtime` feature of ferrin-openai for streaming transcription",
            )
            .into())
        }
    }
}
