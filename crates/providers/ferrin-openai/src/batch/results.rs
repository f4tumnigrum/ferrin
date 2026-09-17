//! Conversion of batch result files to specification items.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::collections::HashMap;
use std::collections::VecDeque;

use ferrin_provider_util::ParseResult;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::batch::BatchError;
use ferrin_spec::batch::BatchItem;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::batch::BatchResultStream;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::ResponseMetadata;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use super::CONVERTIBLE_OUTPUT_TYPES;
use super::api_types::BatchResultLine;
use crate::config::SharedConfig;
use crate::error::OpenAiErrorData;
use crate::error::failed_response_handler;
use crate::responses::api_types::ResponsesResponse;
use crate::responses::output::OutputMapper;
use crate::responses::output::map_finish_reason;
use crate::responses::output::map_usage;
use crate::stream_util::timestamp_from_seconds;

fn error_response(body: Option<&JsonValue>, status_code: u16) -> BatchError {
    let parsed = body.and_then(|body| serde_json::from_value::<OpenAiErrorData>(body.clone()).ok());
    match parsed {
        Some(data) => BatchError {
            message: data.error.message,
            error_type: data.error.error_type,
            code: data.error.code.map(|code| match code {
                JsonValue::String(text) => text,
                other => other.to_string(),
            }),
            status_code: Some(status_code),
        },
        None => BatchError {
            message: format!("OpenAI batch request failed with status code {status_code}"),
            error_type: None,
            code: None,
            status_code: Some(status_code),
        },
    }
}

fn failed(id: String, message: impl Into<String>, code: &str) -> BatchItem<GenerateResult> {
    BatchItem::Failed {
        id,
        error: BatchError {
            message: message.into(),
            error_type: None,
            code: Some(code.to_owned()),
            status_code: None,
        },
        provider_metadata: None,
    }
}

/// Converts a successful Responses body to a generate result.
fn convert_body(config: &SharedConfig, id: String, body: JsonValue) -> BatchItem<GenerateResult> {
    let Ok(response) = serde_json::from_value::<ResponsesResponse>(body.clone()) else {
        return failed(
            id,
            "OpenAI returned an invalid Responses batch result",
            "invalid_response",
        );
    };
    if let Some(error) = &response.error {
        return BatchItem::Failed {
            id,
            error: BatchError {
                message: error
                    .message
                    .clone()
                    .unwrap_or_else(|| "OpenAI response error".to_owned()),
                error_type: None,
                code: error.code.as_ref().map(|code| match code {
                    JsonValue::String(text) => text.clone(),
                    other => other.to_string(),
                }),
                status_code: None,
            },
            provider_metadata: None,
        };
    }
    let Some(output) = &response.output else {
        let message = match response
            .incomplete_details
            .as_ref()
            .and_then(|d| d.reason.as_deref())
        {
            Some(reason) => format!("OpenAI Responses returned no output ({reason})"),
            None => "OpenAI Responses returned no output".to_owned(),
        };
        return failed(id, message, "invalid_response");
    };
    if let Some(item) = output
        .iter()
        .find(|item| !CONVERTIBLE_OUTPUT_TYPES.contains(&item.kind.as_str()))
    {
        return failed(
            id,
            format!(
                "OpenAI returned unsupported batch output type \"{}\"",
                item.kind
            ),
            "unsupported_content",
        );
    }
    let mut mapper = OutputMapper::new(
        config.clone(),
        ToolNameMapping::new(&[], &HashMap::new()),
        None,
    );
    // A batch result has no request definition; paired output proves hosted execution.
    mapper.hosted_shell = output.iter().any(|item| item.kind == "shell_call_output");
    let mut content = Vec::new();
    for item in output {
        content.extend(mapper.map_item(item, true));
    }
    let incomplete = response
        .incomplete_details
        .as_ref()
        .and_then(|d| d.reason.as_deref());
    let finish_reason = map_finish_reason(incomplete, mapper.has_function_call);
    let raw_usage = body.get("usage").and_then(JsonValue::as_object).cloned();
    let usage = response
        .usage
        .as_ref()
        .map(|usage| map_usage(usage, raw_usage))
        .unwrap_or_default();
    let provider_metadata = mapper.response_metadata(
        response.id.as_deref(),
        response.service_tier.as_deref(),
        response.reasoning.as_ref().and_then(|r| r.context.as_ref()),
    );
    let mut result = GenerateResult::new(content, finish_reason);
    result.usage = usage;
    result.provider_metadata = Some(provider_metadata);
    result.response = ResponseMetadata {
        id: response.id.clone(),
        timestamp: timestamp_from_seconds(response.created_at),
        model_id: response.model.clone().map(Into::into),
        headers: None,
        body: Some(body),
    };
    BatchItem::Succeeded { id, result }
}

/// Converts one result line.
#[must_use]
pub fn convert_line(config: &SharedConfig, line: BatchResultLine) -> BatchItemResult {
    let id = line.custom_id;
    let item = if let Some(error) = line.error {
        let error = BatchError {
            message: error
                .message
                .unwrap_or_else(|| "OpenAI batch request failed".to_owned()),
            error_type: None,
            code: error.code,
            status_code: None,
        };
        match error.code.as_deref() {
            Some("batch_cancelled") => BatchItem::Cancelled {
                id,
                error: Some(error),
                provider_metadata: None,
            },
            Some("batch_expired") => BatchItem::Expired {
                id,
                error: Some(error),
                provider_metadata: None,
            },
            _ => BatchItem::Failed {
                id,
                error,
                provider_metadata: None,
            },
        }
    } else if let Some(response) = line.response {
        if !(200..300).contains(&response.status_code) {
            BatchItem::Failed {
                id,
                error: error_response(response.body.as_ref(), response.status_code),
                provider_metadata: None,
            }
        } else {
            convert_body(config, id, response.body.unwrap_or(JsonValue::Null))
        }
    } else {
        failed(
            id,
            "OpenAI returned a batch result without a response or error",
            "invalid_batch_result",
        )
    };
    BatchItemResult::Text(Box::new(item))
}

pub(super) struct ResultsState {
    pub(super) config: SharedConfig,
    pub(super) headers: Headers,
    pub(super) cancellation: CancellationToken,
    pub(super) file_ids: VecDeque<String>,
    pub(super) current: Option<BoxStream<'static, ParseResult<BatchResultLine>>>,
    pub(super) done: bool,
}

pub(super) fn results_stream(state: ResultsState) -> BatchResultStream {
    Box::pin(futures_util::stream::unfold(
        state,
        |mut state| async move {
            loop {
                if state.done {
                    return None;
                }
                if let Some(current) = state.current.as_mut() {
                    match current.next().await {
                        Some(ParseResult::Ok { value, .. }) => {
                            return Some((Ok(convert_line(&state.config, value)), state));
                        }
                        Some(ParseResult::Err { error, .. }) => return Some((Err(error), state)),
                        None => {
                            state.current = None;
                            continue;
                        }
                    }
                }
                let file_id = state.file_ids.pop_front()?;
                let handlers = ResponseHandlers::new(
                    json_lines_response_handler::<BatchResultLine>(),
                    failed_response_handler(),
                );
                let url = state.config.url(&format!("/files/{file_id}/content"));
                match get(
                    state.config.transport.as_ref(),
                    url,
                    state.headers.clone(),
                    &handlers,
                    state.cancellation.clone(),
                )
                .await
                {
                    Ok(response) => state.current = Some(response.value),
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                }
            }
        },
    ))
}
