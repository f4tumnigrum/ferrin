//! Conversion of batch result lines to specification items.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::Content;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::batch::BatchError;
use ferrin_spec::batch::BatchItem;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::language_model::GenerateResult;
use serde::Deserialize;
use serde_json::json;

use super::rpc_error;
use crate::api_types::GenerateContentResponse;
use crate::api_types::RpcStatus;
use crate::config::SharedConfig;
use crate::image::image_result;
use crate::language_model::convert_generate_content_response;
use crate::output::OutputMapper;

/// One line of a batch results file (`{key, response?, error?}`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct BatchResultLine {
    /// Caller-assigned request id.
    pub key: String,
    /// `generateContent` response.
    #[serde(default)]
    pub response: Option<JsonValue>,
    /// Error of a failed request.
    #[serde(default)]
    pub error: Option<RpcStatus>,
}

/// One inlined response (`{metadata: {key}, response?, error?}`).
#[derive(Debug, Clone, Deserialize)]
pub(super) struct InlinedResponse {
    metadata: InlinedMetadata,
    #[serde(default)]
    response: Option<JsonValue>,
    #[serde(default)]
    error: Option<RpcStatus>,
}

#[derive(Debug, Clone, Deserialize)]
struct InlinedMetadata {
    key: String,
}

impl InlinedResponse {
    pub(super) fn into_line(self) -> BatchResultLine {
        BatchResultLine {
            key: self.metadata.key,
            response: self.response,
            error: self.error,
        }
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

fn text(item: BatchItem<GenerateResult>) -> BatchItemResult {
    BatchItemResult::Text(Box::new(item))
}

fn is_image_file(content: &Content) -> bool {
    matches!(
        content,
        Content::File { media_type, .. } if media_type.as_str().starts_with("image/")
    )
}

fn unsupported_kind(content: &Content) -> Option<&'static str> {
    match content {
        Content::Text { .. }
        | Content::Reasoning { .. }
        | Content::Source(_)
        | Content::ToolCall(_)
        | Content::ToolResult(_) => None,
        Content::File { .. } => Some("file"),
        Content::ReasoningFile { .. } => Some("reasoning-file"),
        Content::Custom { .. } => Some("custom"),
        Content::ToolApprovalRequest { .. } => Some("tool-approval-request"),
        #[allow(unreachable_patterns, reason = "Content is non-exhaustive")]
        _ => Some("unknown"),
    }
}

fn blocked(config: &SharedConfig, id: String, response: &JsonValue) -> BatchItemResult {
    let prompt_feedback = response
        .get("promptFeedback")
        .and_then(JsonValue::as_object);
    let block_reason = prompt_feedback
        .and_then(|feedback| feedback.get("blockReason"))
        .and_then(JsonValue::as_str);
    let (message, code) = match block_reason {
        Some(reason) => (
            format!("Google blocked the batch request ({reason})"),
            "prompt_blocked",
        ),
        None => (
            "Google returned a batch response without any candidates".to_owned(),
            "invalid_response",
        ),
    };
    let provider_metadata = prompt_feedback.map(|_| {
        let mut object = JsonObject::new();
        object.insert(
            "promptFeedback".to_owned(),
            json!({"blockReason": block_reason}),
        );
        OutputMapper::new(config.clone(), ToolNameMapping::default()).metadata(object)
    });
    text(BatchItem::Failed {
        id,
        error: BatchError {
            message,
            error_type: block_reason.map(str::to_owned),
            code: Some(code.to_owned()),
            status_code: None,
        },
        provider_metadata,
    })
}

/// Converts one result line.
#[must_use]
pub fn convert_line(config: &SharedConfig, line: BatchResultLine) -> BatchItemResult {
    let id = line.key;
    if let Some(status) = line.error {
        let cancelled = status.status.as_deref() == Some("CANCELLED") || status.code == Some(1);
        let error = rpc_error(&status, "Google batch request failed");
        return text(if cancelled {
            BatchItem::Cancelled {
                id,
                error: Some(error),
                provider_metadata: None,
            }
        } else {
            BatchItem::Failed {
                id,
                error,
                provider_metadata: None,
            }
        });
    }
    let Some(response) = line.response else {
        return text(failed(
            id,
            "Google returned a batch result without a response or error",
            "invalid_batch_result",
        ));
    };
    if response
        .get("candidates")
        .and_then(JsonValue::as_array)
        .is_none_or(Vec::is_empty)
    {
        return blocked(config, id, &response);
    }
    let Ok(body) = serde_json::from_value::<GenerateContentResponse>(response.clone()) else {
        return text(failed(
            id,
            "Google returned an invalid GenerateContent batch result",
            "invalid_response",
        ));
    };
    let result = match convert_generate_content_response(
        config,
        ToolNameMapping::default(),
        Vec::new(),
        &body,
        Some(&response),
    ) {
        Ok(result) => result,
        Err(error) => return text(failed(id, error.to_string(), "invalid_response")),
    };
    if result.content.iter().any(is_image_file) {
        let model_id = result
            .response
            .model_id
            .clone()
            .unwrap_or_else(|| ModelId::new(""));
        return BatchItemResult::Image(Box::new(BatchItem::Succeeded {
            id,
            result: image_result(config, model_id, result, Vec::new()),
        }));
    }
    if let Some(kind) = result.content.iter().find_map(unsupported_kind) {
        return text(failed(
            id,
            format!(
                "Google returned a \"{kind}\" content block, but that content is not supported in text batches"
            ),
            "unsupported_content",
        ));
    }
    text(BatchItem::Succeeded { id, result })
}
