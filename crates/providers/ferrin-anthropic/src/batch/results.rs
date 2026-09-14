//! Conversion of batch result lines to specification items.

use std::collections::HashMap;

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::batch::BatchError;
use ferrin_spec::batch::BatchItem;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::ResponseMetadata;
use serde::Deserialize;

use crate::api_types::AnthropicResponse;
use crate::config::SharedConfig;
use crate::output::MessageMetadata;
use crate::output::OutputMapper;
use crate::output::anthropic_metadata;
use crate::output::container_metadata;
use crate::usage::convert_usage;
use crate::usage::map_stop_reason;

/// One line of the results file.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResultLine {
    /// Request id.
    pub custom_id: String,
    /// `{type: succeeded | errored | canceled | expired, ...}`.
    #[serde(default)]
    pub result: JsonValue,
}

fn invalid(id: String) -> BatchItem<GenerateResult> {
    BatchItem::Failed {
        id,
        error: BatchError {
            message: "Anthropic returned an invalid Message batch result".to_owned(),
            error_type: None,
            code: Some("invalid_response".to_owned()),
            status_code: None,
        },
        provider_metadata: None,
    }
}

/// Converts a complete message of a batch result to a generate result.
#[must_use]
pub fn convert_response(
    config: &SharedConfig,
    response: &AnthropicResponse,
    body: JsonValue,
) -> GenerateResult {
    let mut mapper = OutputMapper::new(config.clone(), ToolNameMapping::new(&[], &HashMap::new()));
    let mut content = Vec::new();
    for block in &response.content {
        content.extend(mapper.map_block(block));
    }
    let raw_usage = body.get("usage").and_then(JsonValue::as_object).cloned();
    let metadata = MessageMetadata {
        usage: raw_usage.clone(),
        stop_sequence: response.stop_sequence.clone(),
        stop_details: response.stop_details.as_ref(),
        input_transformations: response.input_transformations.as_ref(),
        iterations: response.usage.iterations.as_deref(),
        container: response
            .container
            .as_ref()
            .map(|container| container_metadata(container, true)),
        context_management: response.context_management.as_ref(),
    }
    .build(None);
    let mut result = GenerateResult::new(
        content,
        map_stop_reason(response.stop_reason.as_deref(), false),
    );
    result.usage = convert_usage(&response.usage, raw_usage);
    result.provider_metadata = Some(metadata);
    result.response = ResponseMetadata {
        id: response.id.clone(),
        timestamp: None,
        model_id: response.model.clone().map(Into::into),
        headers: None,
        body: Some(body),
    };
    result
}

/// Converts one result line.
#[must_use]
pub fn convert_line(config: &SharedConfig, line: BatchResultLine) -> BatchItemResult {
    let id = line.custom_id;
    let kind = line.result.get("type").and_then(JsonValue::as_str);
    let item = match kind {
        Some("canceled") => BatchItem::Cancelled {
            id,
            error: None,
            provider_metadata: None,
        },
        Some("expired") => BatchItem::Expired {
            id,
            error: None,
            provider_metadata: None,
        },
        Some("errored") => {
            let error = line.result.get("error").cloned().unwrap_or(JsonValue::Null);
            let inner = error.get("error").cloned().unwrap_or(JsonValue::Null);
            let provider_metadata =
                error
                    .get("request_id")
                    .and_then(JsonValue::as_str)
                    .map(|request_id| {
                        let mut meta = JsonObject::new();
                        meta.insert("requestId".to_owned(), JsonValue::from(request_id));
                        anthropic_metadata(meta)
                    });
            BatchItem::Failed {
                id,
                error: BatchError {
                    message: inner
                        .get("message")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("Anthropic batch request failed")
                        .to_owned(),
                    error_type: inner
                        .get("type")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    code: None,
                    status_code: None,
                },
                provider_metadata,
            }
        }
        Some("succeeded") => {
            let message = line
                .result
                .get("message")
                .cloned()
                .unwrap_or(JsonValue::Null);
            match serde_json::from_value::<AnthropicResponse>(message.clone()) {
                Ok(response) if message.is_object() => BatchItem::Succeeded {
                    id,
                    result: convert_response(config, &response, message),
                },
                _ => invalid(id),
            }
        }
        _ => invalid(id),
    };
    BatchItemResult::Text(Box::new(item))
}
