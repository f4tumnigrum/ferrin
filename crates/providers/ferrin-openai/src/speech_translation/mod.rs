//! Speech translation model over the realtime WebSocket (`realtime` feature).

use base64::Engine;
use bytes::Bytes;
use chrono::Utc;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::speech_translation_model::SpeechTranslationModel;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamOptions;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamPart;
use ferrin_spec::speech_translation_model::SpeechTranslationStreamResult;
use serde_json::json;

use crate::config::SharedConfig;
use crate::realtime_ws::WsFlow;
use crate::realtime_ws::WsMachine;
use crate::realtime_ws::WsStreamParams;
use crate::realtime_ws::connect;
use crate::realtime_ws::ws_stream;

/// Transcription model used for the source audio.
const SOURCE_TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";
/// Only supported input sample rate.
const INPUT_SAMPLE_RATE: u32 = 24_000;

/// Builds the `session.update` message.
#[must_use]
pub fn session_update(target_language: &str) -> JsonValue {
    json!({
        "type": "session.update",
        "session": {
            "audio": {
                "input": {
                    "transcription": {"model": SOURCE_TRANSCRIPTION_MODEL},
                    "noise_reduction": JsonValue::Null,
                },
                "output": {"language": target_language},
            }
        }
    })
}

struct Machine {
    warnings: Vec<Warning>,
    source_text: String,
    output_text: String,
}

impl Machine {
    fn finish(&mut self, parts: &mut Vec<SpeechTranslationStreamPart>) {
        if !self.source_text.is_empty() {
            parts.push(SpeechTranslationStreamPart::SourceTranscriptFinal {
                id: None,
                text: self.source_text.clone(),
                start_second: None,
                end_second: None,
                channel_index: None,
                provider_metadata: None,
            });
        }
        if !self.output_text.is_empty() {
            parts.push(SpeechTranslationStreamPart::OutputTextFinal {
                id: None,
                text: self.output_text.clone(),
                provider_metadata: None,
            });
        }
        parts.push(SpeechTranslationStreamPart::Finish {
            source_text: std::mem::take(&mut self.source_text),
            output_text: std::mem::take(&mut self.output_text),
            duration_in_seconds: None,
            usage: None,
            provider_metadata: None,
        });
    }
}

impl WsMachine for Machine {
    type Part = SpeechTranslationStreamPart;

    fn start(&mut self) -> Vec<SpeechTranslationStreamPart> {
        vec![SpeechTranslationStreamPart::StreamStart {
            warnings: std::mem::take(&mut self.warnings),
        }]
    }

    fn raw(&self, raw: JsonValue) -> SpeechTranslationStreamPart {
        SpeechTranslationStreamPart::Raw { raw_value: raw }
    }

    fn handle(
        &mut self,
        event: &JsonValue,
        parts: &mut Vec<SpeechTranslationStreamPart>,
    ) -> WsFlow {
        let kind = event.get("type").and_then(JsonValue::as_str).unwrap_or("");
        let delta = event.get("delta").and_then(JsonValue::as_str).unwrap_or("");
        match kind {
            "session.output_audio.delta" => {
                if !delta.is_empty()
                    && let Ok(audio) = base64::engine::general_purpose::STANDARD.decode(delta)
                {
                    parts.push(SpeechTranslationStreamPart::Audio {
                        id: None,
                        audio: Bytes::from(audio),
                        provider_metadata: None,
                    });
                }
                WsFlow::Continue
            }
            "session.output_transcript.delta" => {
                self.output_text.push_str(delta);
                parts.push(SpeechTranslationStreamPart::OutputTextDelta {
                    id: None,
                    delta: delta.to_owned(),
                    provider_metadata: None,
                });
                WsFlow::Continue
            }
            "session.input_transcript.delta" => {
                self.source_text.push_str(delta);
                parts.push(SpeechTranslationStreamPart::SourceTranscriptDelta {
                    id: None,
                    delta: delta.to_owned(),
                    provider_metadata: None,
                });
                WsFlow::Continue
            }
            "session.closed" => {
                self.finish(parts);
                WsFlow::Finish
            }
            "error" => {
                parts.push(SpeechTranslationStreamPart::Error {
                    error: crate::error::stream_error_for_frame(event),
                });
                WsFlow::Continue
            }
            _ => WsFlow::Continue,
        }
    }

    fn closed(&mut self, parts: &mut Vec<SpeechTranslationStreamPart>) {
        parts.push(SpeechTranslationStreamPart::Error {
            error: StreamError::new(
                "OpenAI realtime translation WebSocket closed unexpectedly before finishing",
            ),
        });
    }

    fn failed(&mut self, error: ProviderError, parts: &mut Vec<SpeechTranslationStreamPart>) {
        parts.push(SpeechTranslationStreamPart::Error {
            error: StreamError::from_provider_error(&error),
        });
    }
}

/// Speech translation model (`<name>.speech-translation`).
#[derive(Debug, Clone)]
pub struct OpenAiSpeechTranslationModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiSpeechTranslationModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("speech-translation"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Returns the shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }
}

impl SpeechTranslationModel for OpenAiSpeechTranslationModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn do_stream(
        &self,
        options: SpeechTranslationStreamOptions,
    ) -> Result<SpeechTranslationStreamResult, ProviderError> {
        if options.target_language.trim().is_empty() {
            return Err(InvalidArgumentError::new(
                "targetLanguage",
                format!(
                    "targetLanguage is required for translation model '{}'",
                    self.model_id
                ),
            )
            .into());
        }
        let format = &options.input_audio_format;
        if format.kind != "audio/pcm" || format.rate.is_some_and(|rate| rate != INPUT_SAMPLE_RATE) {
            return Err(InvalidArgumentError::new(
                "inputAudioFormat",
                "the OpenAI Realtime translation API only supports 24 kHz 16-bit PCM input audio",
            )
            .into());
        }
        let mut warnings = Vec::new();
        if options.source_language.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "sourceLanguage",
                "The OpenAI Realtime translation API auto-detects the source language and does not accept a source language.",
            ));
        }
        if options.output_audio_format.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "outputAudioFormat",
                "The OpenAI Realtime translation API always outputs 24kHz 16-bit PCM audio and does not accept an output audio format.",
            ));
        }
        let session_update = session_update(&options.target_language);
        let url = self.config.websocket_url(
            "/realtime/translations",
            &[("model", self.model_id.as_str())],
        );
        let headers = self.config.headers(&options.headers)?;
        let socket = connect(&url, &headers, &options.cancellation).await?;
        let stream = ws_stream(WsStreamParams {
            socket,
            session_update: session_update.clone(),
            audio: options.audio,
            append_event: "session.input_audio_buffer.append",
            commit_event: json!({"type": "session.close"}),
            include_raw: options.include_raw_chunks,
            cancellation: options.cancellation,
            machine: Machine {
                warnings,
                source_text: String::new(),
                output_text: String::new(),
            },
        });
        Ok(SpeechTranslationStreamResult {
            stream,
            request: RequestMetadata::with_body(session_update),
            response: ResponseMetadata::new(Utc::now(), self.model_id.clone()),
        })
    }
}
