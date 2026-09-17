//! Gemini transcription over Interactions and feature-gated Live streaming.

#[cfg(feature = "realtime")]
mod live;

use base64::Engine;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionResult;
use ferrin_spec::transcription_model::TranscriptionSegment;
use serde::Deserialize;
use serde_json::json;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::options::parse_merged;
use crate::output::OutputMapper;

/// Provider id family.
pub const FAMILY: &str = "transcription";

/// Transcription options (`provider_options["google"]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoogleTranscriptionOptions {
    /// Expected languages (BCP-47).
    #[serde(default)]
    pub language_codes: Option<Vec<String>>,
    /// Custom vocabulary.
    #[serde(default)]
    pub custom_vocabulary: Option<Vec<String>>,
    /// Word-level timestamps.
    #[serde(default)]
    pub word_timestamp: Option<bool>,
    /// Speaker diarization.
    #[serde(default)]
    pub diarization: Option<bool>,
    /// Transcription mode (`SMART`, `VERBATIM`).
    #[serde(default)]
    pub mode: Option<String>,
}

impl GoogleTranscriptionOptions {
    fn merge(mut self, other: Self) -> Self {
        macro_rules! take {
            ($($field:ident),* $(,)?) => {
                $( if other.$field.is_some() { self.$field = other.$field; } )*
            };
        }
        take!(
            language_codes,
            custom_vocabulary,
            word_timestamp,
            diarization,
            mode
        );
        self
    }

    /// `generation_config.transcription_config` of the Interactions request.
    #[must_use]
    pub fn transcription_config(&self) -> Option<JsonObject> {
        let mut config = JsonObject::new();
        if let Some(codes) = &self.language_codes {
            config.insert("language_codes".to_owned(), json!(codes));
        }
        if let Some(vocabulary) = &self.custom_vocabulary {
            config.insert("custom_vocabulary".to_owned(), json!(vocabulary));
        }
        if self.mode.is_some()
            || self.diarization == Some(true)
            || self.word_timestamp == Some(true)
        {
            let mut mode = JsonObject::new();
            mode.insert(
                "type".to_owned(),
                JsonValue::from(
                    self.mode
                        .as_deref()
                        .unwrap_or("VERBATIM")
                        .to_ascii_lowercase(),
                ),
            );
            if self.diarization == Some(true) {
                mode.insert("diarization_mode".to_owned(), JsonValue::from("speaker"));
            }
            if self.word_timestamp == Some(true) {
                mode.insert("timestamp_granularities".to_owned(), json!(["word"]));
            }
            config.insert("mode".to_owned(), JsonValue::Object(mode));
        }
        (!config.is_empty()).then_some(config)
    }
}

#[derive(Debug, Deserialize)]
struct InteractionsResponse {
    #[serde(default)]
    steps: Option<Vec<Step>>,
    #[serde(default)]
    usage: Option<JsonObject>,
}

#[derive(Debug, Deserialize)]
struct Step {
    #[serde(default)]
    content: Option<Vec<StepContent>>,
}

#[derive(Debug, Deserialize)]
struct StepContent {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    annotations: Option<Vec<Annotation>>,
}

#[derive(Debug, Deserialize)]
struct Annotation {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    start_offset: Option<String>,
    #[serde(default)]
    end_offset: Option<String>,
}

fn parse_offset_seconds(offset: Option<&str>) -> Option<f64> {
    let offset = offset?;
    let number = offset.trim_end_matches('s');
    number.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Whether `model_id` only supports streaming (Live API) transcription.
#[must_use]
pub fn is_live_model(model_id: &str) -> bool {
    model_id.contains("-live")
}

/// Transcription model backed by the Interactions API.
#[derive(Debug, Clone)]
pub struct GoogleTranscriptionModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl GoogleTranscriptionModel {
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
    /// Returns [`ProviderError::InvalidArgument`] for live-only model ids and
    /// invalid provider options.
    pub fn prepare_request(
        &self,
        options: &TranscriptionOptions,
    ) -> Result<JsonValue, ProviderError> {
        if is_live_model(self.model_id.as_str()) {
            return Err(InvalidArgumentError::new(
                "model_id",
                format!(
                    "model '{}' only supports streaming transcription over the Live API; use a unary model such as 'gemini-3.5-transcribe'",
                    self.model_id
                ),
            )
            .into());
        }
        let google = parse_merged::<GoogleTranscriptionOptions>(
            &self.config,
            &options.provider_options,
            GoogleTranscriptionOptions::merge,
        )?;
        let mut body = JsonObject::new();
        body.insert("model".to_owned(), JsonValue::from(self.model_id.as_str()));
        body.insert(
            "input".to_owned(),
            json!([{
                "type": "audio",
                "data": base64::engine::general_purpose::STANDARD.encode(&options.audio),
                "mime_type": options.media_type.as_str(),
            }]),
        );
        if let Some(config) = google.transcription_config() {
            body.insert(
                "generation_config".to_owned(),
                json!({"transcription_config": config}),
            );
        }
        Ok(JsonValue::Object(body))
    }
}

impl TranscriptionModel for GoogleTranscriptionModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn supports_stream(&self) -> bool {
        cfg!(feature = "realtime") && is_live_model(self.model_id.as_str())
    }

    #[cfg(feature = "realtime")]
    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(
        &self,
        options: ferrin_spec::transcription_model::TranscriptionStreamOptions,
    ) -> Result<ferrin_spec::transcription_model::TranscriptionStreamResult, ProviderError> {
        live::start(self, options).await
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> Result<TranscriptionResult, ProviderError> {
        let body = self.prepare_request(&options)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<InteractionsResponse>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config.url("/interactions"),
            self.config.headers(&options.headers)?,
            &body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let mut text = String::new();
        let mut segments = Vec::new();
        for step in response.value.steps.iter().flatten() {
            for content in step.content.iter().flatten() {
                if content.kind.as_deref() != Some("text") {
                    continue;
                }
                let Some(content_text) = &content.text else {
                    continue;
                };
                text.push_str(content_text);
                for annotation in content.annotations.iter().flatten() {
                    if annotation.kind.as_deref() != Some("word_info") {
                        continue;
                    }
                    let (Some(word), Some(start), Some(end)) = (
                        &annotation.text,
                        parse_offset_seconds(annotation.start_offset.as_deref()),
                        parse_offset_seconds(annotation.end_offset.as_deref()),
                    ) else {
                        continue;
                    };
                    segments.push(TranscriptionSegment {
                        text: word.clone(),
                        start_second: start,
                        end_second: end,
                    });
                }
            }
        }
        let mapper = OutputMapper::new(self.config.clone(), Default::default());
        let provider_metadata = response.value.usage.map(|usage| {
            let mut object = JsonObject::new();
            object.insert("usage".to_owned(), JsonValue::Object(usage));
            mapper.metadata(object)
        });
        Ok(TranscriptionResult {
            text,
            segments,
            language: None,
            duration_in_seconds: None,
            warnings: Vec::new(),
            request: RequestMetadata::with_body(body),
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            provider_metadata,
        })
    }
}
