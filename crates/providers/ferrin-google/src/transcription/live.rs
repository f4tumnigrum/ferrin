//! Live transcription adapted from Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.); see the root NOTICE.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamPart;
use ferrin_spec::transcription_model::TranscriptionStreamResult;
use serde_json::json;
use tokio::time::Instant;

use super::GoogleTranscriptionModel;
use super::GoogleTranscriptionOptions;
use super::is_live_model;
use crate::config::GoogleConfig;
use crate::live_audio;
use crate::live_audio::FINISH_GRACE;
use crate::live_audio::Mapper;
use crate::options::parse_merged;
use crate::output::OutputMapper;

pub(super) async fn start(
    model: &GoogleTranscriptionModel,
    options: TranscriptionStreamOptions,
) -> Result<TranscriptionStreamResult, ProviderError> {
    if !is_live_model(model.model_id.as_str()) {
        return Err(InvalidArgumentError::new(
            "model_id",
            "streaming transcription requires a live model",
        )
        .into());
    }
    live_audio::validate_format(&options.input_audio_format)?;
    let google = parse_merged::<GoogleTranscriptionOptions>(
        &model.config,
        &options.provider_options,
        GoogleTranscriptionOptions::merge,
    )?;
    let mut transcription = JsonObject::new();
    if let Some(codes) = google.language_codes {
        transcription.insert("languageCodes".to_owned(), json!(codes));
    }
    if let Some(words) = google.custom_vocabulary {
        transcription.insert("customVocabulary".to_owned(), json!(words));
    }
    if let Some(enabled) = google.word_timestamp {
        transcription.insert("wordTimestamp".to_owned(), json!(enabled));
    }
    if let Some(enabled) = google.diarization {
        transcription.insert("diarization".to_owned(), json!(enabled));
    }
    if let Some(mode) = google.mode {
        if !matches!(mode.as_str(), "SMART" | "VERBATIM") {
            return Err(InvalidArgumentError::new(
                "provider_options",
                "transcription mode must be SMART or VERBATIM",
            )
            .into());
        }
        transcription.insert("mode".to_owned(), json!(mode));
    }
    // The reference endpoint only emits final segments without generationConfig.
    let setup = json!({
        "model": GoogleConfig::model_path(model.model_id.as_str()),
        "inputAudioTranscription": transcription,
    });
    let request = RequestMetadata::with_body(setup.clone());
    let mapper = Transcript {
        output: OutputMapper::new(model.config.clone(), Default::default()),
        text: String::new(),
        segment: String::new(),
        interim: String::new(),
        index: 0,
        language: None,
        usage: None,
        ended: false,
        terminal: false,
        deadline: None,
    };
    let (stream, response) = live_audio::start(
        &model.config,
        model.model_id.clone(),
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
    Ok(TranscriptionStreamResult {
        stream,
        request,
        response,
    })
}

struct Transcript {
    output: OutputMapper,
    text: String,
    segment: String,
    interim: String,
    index: u64,
    language: Option<String>,
    usage: Option<JsonValue>,
    ended: bool,
    terminal: bool,
    deadline: Option<Instant>,
}

impl Transcript {
    fn id(&self) -> Option<String> {
        Some(format!("google-segment-{}", self.index))
    }

    fn activity(&mut self) {
        if self.ended {
            self.deadline = Some(Instant::now() + FINISH_GRACE);
        }
    }

    fn segment(&mut self) -> Vec<TranscriptionStreamPart> {
        if self.segment.is_empty() {
            self.segment = std::mem::take(&mut self.interim);
        }
        self.interim.clear();
        if self.segment.is_empty() {
            return Vec::new();
        }
        let text = std::mem::take(&mut self.segment);
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(&text);
        let part = TranscriptionStreamPart::TranscriptFinal {
            id: self.id(),
            text,
            start_second: None,
            end_second: None,
            channel_index: None,
            provider_metadata: None,
        };
        self.index += 1;
        vec![part]
    }
}

impl Mapper for Transcript {
    type Part = TranscriptionStreamPart;

    fn start(&mut self) -> Self::Part {
        Self::Part::StreamStart {
            warnings: Vec::new(),
        }
    }
    fn raw(raw_value: JsonValue) -> Self::Part {
        Self::Part::Raw { raw_value }
    }
    fn error(error: StreamError) -> Self::Part {
        Self::Part::Error { error }
    }

    fn message(&mut self, value: &JsonValue) -> Result<Vec<Self::Part>, Box<StreamError>> {
        let mut parts = Vec::new();
        if let Some(usage) = value.get("usageMetadata") {
            self.usage = Some(usage.clone());
        }
        let content = &value["serverContent"];
        if let Some(text) = content["interimInputTranscription"]["text"]
            .as_str()
            .filter(|text| !text.is_empty())
        {
            self.activity();
            self.interim = text.to_owned();
            parts.push(Self::Part::TranscriptPartial {
                id: self.id(),
                text: text.to_owned(),
                start_second: None,
                duration_in_seconds: None,
                channel_index: None,
                provider_metadata: None,
            });
        }
        if let Some(transcription) = content
            .get("inputTranscription")
            .or_else(|| value.get("inputTranscription"))
        {
            if let Some(language) = transcription["languageCode"].as_str() {
                self.language = Some(language.to_owned());
            }
            if let Some(text) = transcription["text"]
                .as_str()
                .filter(|text| !text.is_empty())
            {
                self.activity();
                self.interim.clear();
                self.segment.push_str(text);
                parts.push(Self::Part::TranscriptDelta {
                    id: self.id(),
                    delta: text.to_owned(),
                    provider_metadata: None,
                });
            }
            if transcription["finished"].as_bool() == Some(true) {
                parts.extend(self.segment());
            }
        }
        let turn_complete = content["turnComplete"].as_bool() == Some(true);
        if turn_complete {
            parts.extend(self.segment());
        }
        let status = content["interactionStatus"].as_str();
        self.terminal = self.ended
            && (matches!(status, Some("IDLE" | "REQUIRES_ACTION"))
                || (turn_complete && status.is_none()));
        Ok(parts)
    }

    fn audio_ended(&mut self) {
        self.ended = true;
        self.activity();
    }
    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
    fn can_close(&self) -> bool {
        self.ended
    }
    fn complete(&self) -> bool {
        self.terminal
    }

    fn finish(&mut self) -> Vec<Self::Part> {
        let mut parts = self.segment();
        let provider_metadata = self.usage.take().map(|usage| {
            self.output
                .metadata(JsonObject::from_iter([("usageMetadata".to_owned(), usage)]))
        });
        parts.push(Self::Part::Finish {
            text: std::mem::take(&mut self.text),
            segments: Vec::new(),
            language: self.language.take(),
            duration_in_seconds: None,
            provider_metadata,
        });
        parts
    }
}
