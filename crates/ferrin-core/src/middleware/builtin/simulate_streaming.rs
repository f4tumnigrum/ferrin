//! Streaming simulated from a complete response.

use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::StreamResult;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::GenerateResult;
use futures_util::stream;

use crate::middleware::LanguageModelMiddleware;
use crate::middleware::MiddlewareContext;
use crate::middleware::StreamNext;

/// Middleware created by [`simulate_streaming`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SimulateStreaming;

/// Serves `do_stream` by calling `do_generate` on the wrapped model and
/// expanding the result into `stream-start`, `response-metadata`, one
/// start/delta/end triple per text or reasoning part (empty text parts are
/// skipped), the other parts as is, and `finish`.
#[must_use]
pub fn simulate_streaming() -> SimulateStreaming {
    SimulateStreaming
}

/// Expands a complete result into stream parts.
#[must_use]
pub fn simulate_parts(result: &GenerateResult) -> Vec<StreamPart> {
    let mut parts = vec![
        StreamPart::StreamStart {
            warnings: result.warnings.clone(),
        },
        StreamPart::ResponseMetadata {
            id: result.response.id.clone(),
            timestamp: result.response.timestamp,
            model_id: result.response.model_id.clone(),
        },
    ];
    let mut next_id = 0u32;
    for part in &result.content {
        match part {
            Content::Text { text, .. } => {
                if text.is_empty() {
                    continue;
                }
                let id = PartId::new(next_id.to_string());
                next_id += 1;
                parts.push(StreamPart::TextStart {
                    id: id.clone(),
                    provider_metadata: None,
                });
                parts.push(StreamPart::TextDelta {
                    id: id.clone(),
                    delta: text.clone(),
                    provider_metadata: None,
                });
                parts.push(StreamPart::TextEnd {
                    id,
                    provider_metadata: None,
                });
            }
            Content::Reasoning {
                text,
                provider_metadata,
            } => {
                let id = PartId::new(next_id.to_string());
                next_id += 1;
                parts.push(StreamPart::ReasoningStart {
                    id: id.clone(),
                    provider_metadata: provider_metadata.clone(),
                });
                parts.push(StreamPart::ReasoningDelta {
                    id: id.clone(),
                    delta: text.clone(),
                    provider_metadata: None,
                });
                parts.push(StreamPart::ReasoningEnd {
                    id,
                    provider_metadata: None,
                });
            }
            Content::ReasoningFile {
                data,
                media_type,
                provider_metadata,
            } => parts.push(StreamPart::ReasoningFile {
                data: data.clone(),
                media_type: media_type.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::File {
                data,
                media_type,
                filename,
                provider_metadata,
            } => parts.push(StreamPart::File {
                data: data.clone(),
                media_type: media_type.clone(),
                filename: filename.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::Custom {
                kind,
                provider_metadata,
            } => parts.push(StreamPart::Custom {
                kind: kind.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            Content::Source(source) => parts.push(StreamPart::Source(source.clone())),
            Content::ToolCall(call) => parts.push(StreamPart::ToolCall(call.clone())),
            Content::ToolResult(result) => parts.push(StreamPart::ToolResult(result.clone())),
            Content::ToolApprovalRequest {
                approval_id,
                tool_call_id,
                provider_metadata,
            } => parts.push(StreamPart::ToolApprovalRequest {
                approval_id: approval_id.clone(),
                tool_call_id: tool_call_id.clone(),
                provider_metadata: provider_metadata.clone(),
            }),
            #[allow(unreachable_patterns, reason = "Content is non-exhaustive")]
            _ => {}
        }
    }
    parts.push(StreamPart::Finish {
        finish_reason: result.finish_reason.clone(),
        usage: result.usage.clone(),
        provider_metadata: result.provider_metadata.clone(),
    });
    parts
}

impl LanguageModelMiddleware for SimulateStreaming {
    fn wrap_stream<'a>(
        &'a self,
        options: CallOptions,
        _next: StreamNext<'a>,
        ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<StreamResult, ProviderError>> {
        Box::pin(async move {
            let result = ctx.model.do_generate(options).await?;
            let parts = simulate_parts(&result);
            Ok(StreamResult {
                stream: Box::pin(stream::iter(parts)),
                request: result.request,
                response: result.response,
            })
        })
    }
}
