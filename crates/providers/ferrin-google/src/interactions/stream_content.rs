//! Complete content-to-stream conversion, derived from Vercel AI SDK (Apache-2.0).

use ferrin_spec::Content;
use ferrin_spec::PartId;
use ferrin_spec::language_model::StreamPart;

pub(super) fn content_parts(content: Content, id: &PartId) -> Vec<StreamPart> {
    match content {
        Content::Text {
            text,
            provider_metadata,
        } => vec![
            StreamPart::TextStart {
                id: id.clone(),
                provider_metadata: provider_metadata.clone(),
            },
            StreamPart::text_delta(id.clone(), text),
            StreamPart::TextEnd {
                id: id.clone(),
                provider_metadata,
            },
        ],
        Content::Reasoning {
            text,
            provider_metadata,
        } => vec![
            StreamPart::ReasoningStart {
                id: id.clone(),
                provider_metadata: provider_metadata.clone(),
            },
            StreamPart::ReasoningDelta {
                id: id.clone(),
                delta: text,
                provider_metadata: None,
            },
            StreamPart::ReasoningEnd {
                id: id.clone(),
                provider_metadata,
            },
        ],
        Content::ToolCall(call) => vec![
            StreamPart::ToolInputStart {
                id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                provider_executed: call.provider_executed,
                dynamic: call.dynamic,
                title: None,
                provider_metadata: call.provider_metadata.clone(),
            },
            StreamPart::ToolInputDelta {
                id: call.tool_call_id.clone(),
                delta: call.input.clone(),
                provider_metadata: None,
            },
            StreamPart::ToolInputEnd {
                id: call.tool_call_id.clone(),
                provider_metadata: None,
            },
            StreamPart::ToolCall(call),
        ],
        Content::ToolResult(result) => vec![StreamPart::ToolResult(result)],
        Content::File {
            data,
            media_type,
            filename,
            provider_metadata,
        } => vec![StreamPart::File {
            data,
            media_type,
            filename,
            provider_metadata,
        }],
        Content::Source(source) => vec![StreamPart::Source(source)],
        Content::Custom {
            kind,
            provider_metadata,
        } => vec![StreamPart::Custom {
            kind,
            provider_metadata,
        }],
        _ => Vec::new(),
    }
}
