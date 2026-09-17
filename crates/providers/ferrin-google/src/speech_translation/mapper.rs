//! Stateful translation turns, usage deltas and continuous PCM completion.
//!
//! Derived from Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.);
//! see the root NOTICE.

use base64::Engine;
use bytes::Bytes;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::Warning;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
use ferrin_spec::speech_translation_model::SpeechTranslationUsage;
use serde_json::json;
use tokio::time::Instant;

use crate::config::SharedConfig;
use crate::live_audio::FINISH_GRACE;
use crate::live_audio::Mapper;
use crate::output::OutputMapper;

pub(super) struct Translation {
    output: OutputMapper,
    warnings: Vec<Warning>,
    source: String,
    translated: String,
    source_turn: String,
    translated_turn: String,
    index: u64,
    ended: bool,
    open_turn: bool,
    saw_complete: bool,
    silent_samples: usize,
    terminal: bool,
    deadline: Option<Instant>,
    usage: SpeechTranslationUsage,
    raw_usage: Option<JsonValue>,
}

impl Translation {
    pub(super) fn new(config: SharedConfig, warnings: Vec<Warning>) -> Self {
        Self {
            output: OutputMapper::new(config, Default::default()),
            warnings,
            source: String::new(),
            translated: String::new(),
            source_turn: String::new(),
            translated_turn: String::new(),
            index: 0,
            ended: false,
            open_turn: false,
            saw_complete: false,
            silent_samples: 0,
            terminal: false,
            deadline: None,
            usage: SpeechTranslationUsage::default(),
            raw_usage: None,
        }
    }

    fn id(&self) -> Option<String> {
        Some(format!("google-item-{}", self.index))
    }

    fn activity(&mut self) {
        self.open_turn = true;
        self.silent_samples = 0;
        self.deadline = None;
    }

    fn finish_turn(&mut self) -> Vec<SpeechTranslationStreamPart> {
        let mut parts = Vec::new();
        if !self.source_turn.is_empty() {
            let text = std::mem::take(&mut self.source_turn);
            self.source.push_str(&text);
            parts.push(SpeechTranslationStreamPart::SourceTranscriptFinal {
                id: self.id(),
                text,
                start_second: None,
                end_second: None,
                channel_index: None,
                provider_metadata: None,
            });
        }
        if !self.translated_turn.is_empty() {
            let text = std::mem::take(&mut self.translated_turn);
            self.translated.push_str(&text);
            parts.push(SpeechTranslationStreamPart::OutputTextFinal {
                id: self.id(),
                text,
                provider_metadata: None,
            });
        }
        self.index += 1;
        parts
    }

    fn audio_metadata(&self) -> ProviderMetadata {
        self.output.metadata(JsonObject::from_iter([
            ("sampleRate".to_owned(), json!(24_000)),
            ("mimeType".to_owned(), json!("audio/pcm;rate=24000")),
        ]))
    }

    fn add_usage(&mut self, value: &JsonValue) {
        for (key, target) in [
            ("promptTokensDetails", &mut self.usage.input_audio_tokens),
            ("responseTokensDetails", &mut self.usage.output_audio_tokens),
        ] {
            for detail in value[key].as_array().into_iter().flatten() {
                if detail["modality"].as_str() == Some("AUDIO")
                    && let Some(tokens) = detail["tokenCount"].as_u64()
                {
                    *target = Some(target.unwrap_or_default().saturating_add(tokens));
                }
            }
        }
        // TEXT prompt details are internal translation context, not billed input text.
        self.raw_usage = Some(value.clone());
    }
}

impl Mapper for Translation {
    type Part = SpeechTranslationStreamPart;

    fn start(&mut self) -> Self::Part {
        Self::Part::StreamStart {
            warnings: std::mem::take(&mut self.warnings),
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
            self.add_usage(usage);
        }
        let content = &value["serverContent"];
        if let Some(text) = content
            .get("inputTranscription")
            .or_else(|| value.get("inputTranscription"))
            .and_then(|transcription| transcription["text"].as_str())
            .filter(|text| !text.is_empty())
        {
            self.activity();
            self.source_turn.push_str(text);
            parts.push(Self::Part::SourceTranscriptDelta {
                id: self.id(),
                delta: text.to_owned(),
                provider_metadata: None,
            });
        }
        let mut silent_samples = 0;
        for part in content["modelTurn"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(data) = part["inlineData"]["data"]
                .as_str()
                .filter(|data| !data.is_empty())
            {
                let audio = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| StreamError::new("Google Live returned invalid audio base64"))?;
                if audio.len() % 2 != 0 {
                    return Err(Box::new(StreamError::new(
                        "Google Live returned incomplete PCM samples",
                    )));
                }
                let is_silent =
                    audio.as_chunks::<2>().0.iter().all(|sample| {
                        i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() <= 128
                    });
                if self.ended && is_silent {
                    silent_samples += audio.len() / 2;
                } else {
                    self.activity();
                    silent_samples = 0;
                }
                parts.push(Self::Part::Audio {
                    id: self.id(),
                    audio: Bytes::from(audio),
                    provider_metadata: Some(self.audio_metadata()),
                });
            }
        }
        if let Some(text) = content["outputTranscription"]["text"]
            .as_str()
            .filter(|text| !text.is_empty())
        {
            self.activity();
            silent_samples = 0;
            self.translated_turn.push_str(text);
            parts.push(Self::Part::OutputTextDelta {
                id: self.id(),
                delta: text.to_owned(),
                provider_metadata: None,
            });
        }
        self.silent_samples += silent_samples;
        if self.silent_samples >= 24_000 {
            self.terminal = true;
        }
        if content["turnComplete"].as_bool() == Some(true) {
            parts.extend(self.finish_turn());
            self.open_turn = false;
            self.saw_complete = true;
            if self.ended {
                self.deadline = Some(Instant::now() + FINISH_GRACE);
            }
        }
        Ok(parts)
    }

    fn audio_ended(&mut self) {
        self.ended = true;
        if self.saw_complete && !self.open_turn {
            self.deadline = Some(Instant::now() + FINISH_GRACE);
        }
    }
    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
    fn can_close(&self) -> bool {
        self.ended && self.deadline.is_some()
    }
    fn complete(&self) -> bool {
        self.terminal
    }

    fn finish(&mut self) -> Vec<Self::Part> {
        let mut parts = self.finish_turn();
        let provider_metadata = self.raw_usage.take().map(|usage| {
            self.output
                .metadata(JsonObject::from_iter([("usageMetadata".to_owned(), usage)]))
        });
        let usage = (self.usage != SpeechTranslationUsage::default()).then_some(self.usage);
        parts.push(Self::Part::Finish {
            source_text: std::mem::take(&mut self.source),
            output_text: std::mem::take(&mut self.translated),
            duration_in_seconds: None,
            usage,
            provider_metadata,
        });
        parts
    }
}
