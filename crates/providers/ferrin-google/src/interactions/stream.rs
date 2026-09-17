//! Interactions SSE state machine, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use futures_util::StreamExt;
use std::collections::BTreeMap;
use std::collections::HashSet;

use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_spec::Content;
use ferrin_spec::JsonValue;
use ferrin_spec::PartId;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::StreamPart;
use serde_json::json;

use crate::config::SharedConfig;

use super::output;
use super::stream_content::content_parts;

struct OpenStep {
    step: JsonValue,
    id: PartId,
    text_open: bool,
    arguments: String,
}

pub(super) struct State {
    config: SharedConfig,
    aliases: BTreeMap<String, String>,
    open: BTreeMap<u64, OpenStep>,
    interaction: JsonValue,
    terminal: bool,
    function: bool,
    emitted_sources: HashSet<String>,
}

impl State {
    pub(super) fn new(config: SharedConfig, aliases: BTreeMap<String, String>) -> Self {
        Self {
            config,
            aliases,
            open: BTreeMap::new(),
            interaction: json!({}),
            terminal: false,
            function: false,
            emitted_sources: HashSet::new(),
        }
    }

    pub(super) fn with_interaction(mut self, interaction: JsonValue) -> Self {
        self.interaction = interaction;
        self
    }

    fn event(&mut self, event: JsonValue) -> Result<Vec<StreamPart>, ProviderError> {
        let mut parts = Vec::new();
        if !event.is_object() || event["event_type"].as_str().is_none() {
            return Err(super::bad_response(
                "interactions event must be a typed object",
            ));
        }
        match event["event_type"].as_str().unwrap_or_default() {
            "interaction.created" => {
                self.interaction = event["interaction"].clone();
                parts.push(StreamPart::ResponseMetadata {
                    id: self.interaction["id"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .map(str::to_owned),
                    timestamp: None,
                    model_id: self.interaction["model"].as_str().map(Into::into),
                });
            }
            "step.start" => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| super::bad_response("interactions step omitted index"))?;
                if self.open.contains_key(&index) {
                    return Err(super::bad_response("interactions step opened twice"));
                }
                let step = event["step"].clone();
                if !step.is_object() || step["type"].as_str().is_none() {
                    return Err(super::bad_response(
                        "interactions step must be a typed object",
                    ));
                }
                let mut text_open = false;
                let id: PartId = format!("step-{index}").into();
                let metadata = Some(output::part_metadata(
                    &self.config,
                    &step,
                    self.interaction["id"].as_str(),
                ));
                if step["type"] == "thought" {
                    parts.push(StreamPart::ReasoningStart {
                        id: id.clone(),
                        provider_metadata: metadata.clone(),
                    });
                    for item in step["summary"].as_array().into_iter().flatten() {
                        if let Some(text) = item["text"].as_str() {
                            parts.push(StreamPart::ReasoningDelta {
                                id: id.clone(),
                                delta: text.to_owned(),
                                provider_metadata: None,
                            });
                        }
                    }
                }
                if step["type"] == "model_output" {
                    for content in output::step(
                        &self.config,
                        &step,
                        self.interaction["id"].as_str(),
                        &self.aliases,
                    )? {
                        if let Content::Text {
                            text,
                            provider_metadata,
                        } = content
                        {
                            if !text_open {
                                parts.push(StreamPart::TextStart {
                                    id: id.clone(),
                                    provider_metadata,
                                });
                                text_open = true;
                            }
                            parts.push(StreamPart::text_delta(id.clone(), text));
                        } else {
                            parts.extend(content_parts(content, &id));
                        }
                    }
                }
                if step["type"] == "function_call" {
                    let call_id = step["id"].as_str().ok_or_else(|| {
                        super::bad_response("interactions function call omitted id")
                    })?;
                    let name = step["name"].as_str().ok_or_else(|| {
                        super::bad_response("interactions function call omitted name")
                    })?;
                    parts.push(StreamPart::ToolInputStart {
                        id: call_id.into(),
                        tool_name: name.into(),
                        provider_executed: false,
                        dynamic: false,
                        title: None,
                        provider_metadata: metadata,
                    });
                    self.function = true;
                }
                let arguments = String::new();
                self.open.insert(
                    index,
                    OpenStep {
                        step,
                        id,
                        text_open,
                        arguments,
                    },
                );
            }
            "step.delta" => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| super::bad_response("interactions delta omitted index"))?;
                let open = self
                    .open
                    .get_mut(&index)
                    .ok_or_else(|| super::bad_response("interactions delta before step start"))?;
                let delta = &event["delta"];
                if !delta.is_object() || delta["type"].as_str().is_none() {
                    return Err(super::bad_response(
                        "interactions delta must be a typed object",
                    ));
                }
                match delta["type"].as_str().unwrap_or_default() {
                    "text" if open.step["type"] == "model_output" => {
                        if !open.text_open {
                            parts.push(StreamPart::TextStart {
                                id: open.id.clone(),
                                provider_metadata: None,
                            });
                            open.text_open = true;
                        }
                        parts.push(StreamPart::text_delta(
                            open.id.clone(),
                            delta["text"].as_str().unwrap_or_default(),
                        ));
                    }
                    "thought_summary" if open.step["type"] == "thought" => {
                        parts.push(StreamPart::ReasoningDelta {
                            id: open.id.clone(),
                            delta: delta["content"]["text"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            provider_metadata: None,
                        });
                    }
                    "thought_signature" if open.step["type"] == "thought" => {
                        open.step["signature"] = delta["signature"].clone()
                    }
                    "arguments_delta" if open.step["type"] == "function_call" => {
                        if delta
                            .get("arguments")
                            .is_some_and(|value| !value.is_null() && !value.is_string())
                        {
                            return Err(super::bad_response(
                                "interactions arguments delta must be a string",
                            ));
                        }
                        let delta_text = delta["arguments"].as_str().unwrap_or_default();
                        open.arguments.push_str(delta_text);
                        if let Some(signature) = delta.get("signature") {
                            open.step["signature"] = signature.clone();
                        }
                        parts.push(StreamPart::ToolInputDelta {
                            id: open.step["id"].as_str().unwrap_or_default().into(),
                            delta: delta_text.to_owned(),
                            provider_metadata: None,
                        });
                    }
                    "image"
                    | "audio"
                    | "video"
                    | "document"
                    | "text_annotation"
                    | "text_annotation_delta"
                        if open.step["type"] == "model_output" =>
                    {
                        let metadata = output::part_metadata(
                            &self.config,
                            &open.step,
                            self.interaction["id"].as_str(),
                        );
                        let mut block = delta.clone();
                        if matches!(
                            delta["type"].as_str(),
                            Some("text_annotation" | "text_annotation_delta")
                        ) {
                            block["type"] = json!("text");
                        }
                        for content in output::block(&self.config, &block, &metadata)? {
                            if !matches!(content, Content::Text { .. }) {
                                parts.extend(content_parts(content, &open.id));
                            }
                        }
                    }
                    _ if delta["type"] == open.step["type"] => {
                        if let (Some(target), Some(delta)) =
                            (open.step.as_object_mut(), delta.as_object())
                        {
                            target.extend(
                                delta
                                    .iter()
                                    .filter(|(key, _)| {
                                        !matches!(key.as_str(), "type" | "id" | "name")
                                    })
                                    .map(|(key, value)| (key.clone(), value.clone())),
                            );
                        }
                    }
                    "text" | "thought_summary" | "thought_signature" | "arguments_delta"
                    | "image" | "audio" | "video" | "document" => {
                        return Err(super::bad_response(
                            "interactions delta does not match its step",
                        ));
                    }
                    _ => {}
                }
            }
            "step.stop" => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| super::bad_response("interactions stop omitted index"))?;
                let mut open = self
                    .open
                    .remove(&index)
                    .ok_or_else(|| super::bad_response("interactions stop before step start"))?;
                let metadata = Some(output::part_metadata(
                    &self.config,
                    &open.step,
                    self.interaction["id"].as_str(),
                ));
                match open.step["type"].as_str().unwrap_or_default() {
                    "model_output" if open.text_open => parts.push(StreamPart::TextEnd {
                        id: open.id,
                        provider_metadata: metadata,
                    }),
                    "model_output" => {}
                    "thought" => parts.push(StreamPart::ReasoningEnd {
                        id: open.id,
                        provider_metadata: metadata,
                    }),
                    "function_call" => {
                        open.step["arguments"] = if open.arguments.is_empty() {
                            open.step
                                .get("arguments")
                                .filter(|value| !value.is_null())
                                .cloned()
                                .unwrap_or_else(|| json!({}))
                        } else {
                            serde_json::from_str(&open.arguments).map_err(|_| {
                                super::bad_response("invalid interactions function arguments")
                            })?
                        };
                        parts.push(StreamPart::ToolInputEnd {
                            id: open.step["id"].as_str().unwrap_or_default().into(),
                            provider_metadata: metadata,
                        });
                        for content in output::step(
                            &self.config,
                            &open.step,
                            self.interaction["id"].as_str(),
                            &self.aliases,
                        )? {
                            if let Content::ToolCall(call) = content {
                                parts.push(StreamPart::ToolCall(call));
                            }
                        }
                    }
                    _ => {
                        for content in output::step(
                            &self.config,
                            &open.step,
                            self.interaction["id"].as_str(),
                            &self.aliases,
                        )? {
                            parts.extend(content_parts(content, &open.id));
                        }
                    }
                }
            }
            "interaction.completed" | "interaction.complete" => {
                if !self.open.is_empty() {
                    return Err(super::bad_response(
                        "interactions completed with open steps",
                    ));
                }
                let interaction = event["interaction"]
                    .as_object()
                    .ok_or_else(|| super::bad_response("terminal interaction omitted resource"))?;
                if !matches!(
                    interaction.get("status").and_then(JsonValue::as_str),
                    Some("completed" | "failed" | "cancelled" | "incomplete" | "requires_action")
                ) {
                    return Err(super::bad_response(
                        "terminal interaction has invalid status",
                    ));
                }
                self.interaction = JsonValue::Object(interaction.clone());
                self.terminal = true;
            }
            "error" => {
                return Err(super::bad_response(
                    "google interactions stream reported an error",
                ));
            }
            _ => {}
        }
        parts.retain(|part| match part {
            StreamPart::Source(source) => self.emitted_sources.insert(super::sources::key(source)),
            _ => true,
        });
        Ok(parts)
    }
}

pub(super) fn terminal_chunks(
    chunks: ferrin_spec::BoxStream<'static, ParseResult<JsonValue>>,
) -> ferrin_spec::BoxStream<'static, ParseResult<JsonValue>> {
    Box::pin(futures_util::stream::unfold(
        (chunks, false),
        |(mut chunks, terminal)| async move {
            if terminal {
                return None;
            }
            let next = chunks.next().await?;
            let terminal = matches!(&next, ParseResult::Ok { value, .. } if matches!(value["event_type"].as_str(), Some("interaction.completed" | "interaction.complete" | "error")));
            Some((next, (chunks, terminal)))
        },
    ))
}

impl StreamMachine for State {
    type Chunk = JsonValue;

    fn handle(&mut self, chunk: ParseResult<JsonValue>, include_raw: bool) -> Vec<StreamPart> {
        let (value, raw) = match chunk {
            ParseResult::Ok { value, raw } => (value, raw),
            ParseResult::Err { error, .. } => return vec![StreamPart::error(&error)],
        };
        let mut parts = if include_raw {
            vec![StreamPart::Raw { raw_value: raw }]
        } else {
            Vec::new()
        };
        match self.event(value) {
            Ok(events) => parts.extend(events),
            Err(error) => parts.push(StreamPart::error(&error)),
        }
        parts
    }

    fn finish(self) -> Vec<StreamPart> {
        if !self.terminal {
            return vec![StreamPart::error(&super::bad_response(
                "interactions stream ended before a terminal event",
            ))];
        }
        vec![StreamPart::Finish {
            finish_reason: output::finish(
                self.interaction["status"].as_str().unwrap_or("completed"),
                self.function,
            ),
            usage: output::usage(&self.interaction["usage"]),
            provider_metadata: Some(output::response_metadata(&self.config, &self.interaction)),
        }]
    }
}
