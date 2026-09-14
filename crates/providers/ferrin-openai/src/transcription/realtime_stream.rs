//! Streaming transcription over the realtime WebSocket (`realtime` feature).

use chrono::Utc;
use ferrin_spec::JsonValue;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::ResponseMetadata;
use ferrin_spec::language_model::StreamError;
use ferrin_spec::transcription_model::TranscriptionStreamOptions;
use ferrin_spec::transcription_model::TranscriptionStreamPart;
use ferrin_spec::transcription_model::TranscriptionStreamResult;
use serde_json::json;

use super::OpenAiTranscriptionModel;
use super::TranscriptionProviderOptions;
use crate::realtime_ws::WsFlow;
use crate::realtime_ws::WsMachine;
use crate::realtime_ws::WsStreamParams;
use crate::realtime_ws::connect;
use crate::realtime_ws::ws_stream;

/// Builds the `session.update` message.
#[must_use]
pub fn session_update(
    model_id: &str,
    format: &ferrin_spec::AudioFormat,
    options: Option<&TranscriptionProviderOptions>,
) -> JsonValue {
    let mut audio_format = json!({"type": format.kind});
    if let Some(rate) = format.rate
        && let Some(object) = audio_format.as_object_mut()
    {
        object.insert("rate".to_owned(), JsonValue::from(rate));
    }
    let mut transcription = json!({"model": model_id});
    if let Some(object) = transcription.as_object_mut() {
        if let Some(language) = options.and_then(|o| o.language.as_deref()) {
            object.insert("language".to_owned(), JsonValue::from(language));
        }
        if let Some(delay) = options
            .and_then(|o| o.streaming.as_ref())
            .and_then(|s| s.delay.as_deref())
        {
            object.insert("delay".to_owned(), JsonValue::from(delay));
        }
    }
    let mut session = json!({
        "type": "transcription",
        "audio": {
            "input": {
                "format": audio_format,
                "transcription": transcription,
                "turn_detection": JsonValue::Null,
            }
        }
    });
    if let Some(include) = options
        .and_then(|o| o.streaming.as_ref())
        .and_then(|s| s.include.clone())
        && let Some(object) = session.as_object_mut()
    {
        object.insert("include".to_owned(), json!(include));
    }
    json!({"type": "session.update", "session": session})
}

struct Machine {
    warnings: Vec<Warning>,
    language: Option<String>,
}

impl WsMachine for Machine {
    type Part = TranscriptionStreamPart;

    fn start(&mut self) -> Vec<TranscriptionStreamPart> {
        vec![TranscriptionStreamPart::StreamStart {
            warnings: std::mem::take(&mut self.warnings),
        }]
    }

    fn raw(&self, raw: JsonValue) -> TranscriptionStreamPart {
        TranscriptionStreamPart::Raw { raw_value: raw }
    }

    fn handle(&mut self, event: &JsonValue, parts: &mut Vec<TranscriptionStreamPart>) -> WsFlow {
        let kind = event.get("type").and_then(JsonValue::as_str).unwrap_or("");
        let item_id = event
            .get("item_id")
            .and_then(JsonValue::as_str)
            .map(str::to_owned);
        match kind {
            "conversation.item.input_audio_transcription.delta" => {
                parts.push(TranscriptionStreamPart::TranscriptDelta {
                    id: item_id,
                    delta: event
                        .get("delta")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    provider_metadata: None,
                });
                WsFlow::Continue
            }
            "conversation.item.input_audio_transcription.completed" => {
                let text = event
                    .get("transcript")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if item_id.is_some() {
                    parts.push(TranscriptionStreamPart::TranscriptFinal {
                        id: item_id,
                        text: text.clone(),
                        start_second: None,
                        end_second: None,
                        channel_index: None,
                        provider_metadata: None,
                    });
                }
                parts.push(TranscriptionStreamPart::Finish {
                    text,
                    segments: Vec::new(),
                    language: self.language.clone(),
                    duration_in_seconds: None,
                    provider_metadata: None,
                });
                WsFlow::Finish
            }
            "error" => {
                parts.push(TranscriptionStreamPart::Error {
                    error: crate::error::stream_error_for_frame(event),
                });
                WsFlow::Finish
            }
            _ => WsFlow::Continue,
        }
    }

    fn closed(&mut self, _parts: &mut Vec<TranscriptionStreamPart>) {}

    fn failed(&mut self, error: ProviderError, parts: &mut Vec<TranscriptionStreamPart>) {
        parts.push(TranscriptionStreamPart::Error {
            error: StreamError::from_provider_error(&error),
        });
    }
}

/// Opens the realtime transcription session.
pub(super) async fn stream(
    model: &OpenAiTranscriptionModel,
    options: TranscriptionStreamOptions,
) -> Result<TranscriptionStreamResult, ProviderError> {
    let config = model.config();
    let openai = model.provider_options(&options.provider_options)?;
    let mut warnings = Vec::new();
    if let Some(openai) = &openai {
        for (present, option) in [
            (openai.include.is_some(), "include"),
            (openai.prompt.is_some(), "prompt"),
            (openai.temperature.is_some(), "temperature"),
            (
                openai.timestamp_granularities.is_some(),
                "timestampGranularities",
            ),
        ] {
            if present {
                warnings.push(Warning::unsupported_with_details(
                    format!("providerOptions.{}.{option}", config.provider_options_key),
                    format!("OpenAI streaming transcription does not support {option}."),
                ));
            }
        }
    }
    let session_update = session_update(
        model.model_id.as_str(),
        &options.input_audio_format,
        openai.as_ref(),
    );
    let url = config.websocket_url("/realtime", &[("intent", "transcription")]);
    let headers = config.headers(&options.headers)?;
    let socket = connect(&url, &headers, &options.cancellation).await?;
    let machine = Machine {
        warnings,
        language: openai.as_ref().and_then(|o| o.language.clone()),
    };
    let stream = ws_stream(WsStreamParams {
        socket,
        session_update: session_update.clone(),
        audio: options.audio,
        append_event: "input_audio_buffer.append",
        commit_event: json!({"type": "input_audio_buffer.commit"}),
        include_raw: options.include_raw_chunks,
        cancellation: options.cancellation,
        machine,
    });
    Ok(TranscriptionStreamResult {
        stream,
        request: RequestMetadata::with_body(session_update),
        response: ResponseMetadata::new(Utc::now(), model.model_id.clone()),
    })
}
