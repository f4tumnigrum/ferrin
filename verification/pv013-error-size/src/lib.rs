//! PV-013: size of the designed core `Error` enum
//! (docs/01-architecture/12-error-model.md) with and without boxing.
#![allow(dead_code)]

use std::time::Duration;

use http::StatusCode;
use url::Url;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy)] pub enum RetryReason { MaxRetriesExceeded, ErrorNotRetryable, Aborted }
#[derive(Debug, Clone, Copy)] pub enum TimeoutScope { Total, Step, FirstChunk, Chunk, Tool }
#[derive(Debug, Clone, Copy)] pub enum FinishReason { Stop, Length, ContentFilter, ToolCalls, Error, Other, Unknown }
#[derive(Debug, Clone, Copy)] pub enum ModelKind { Language, Embedding, Image }
#[derive(Debug, Clone)] pub struct ToolName(String);
#[derive(Debug, Clone)] pub struct ToolCallId(String);
#[derive(Debug, Clone)] pub struct ApprovalId(String);
#[derive(Debug, Clone)] pub struct ProviderId(String);
#[derive(Debug, Clone, Default)] pub struct Usage { pub input: Option<u64>, pub output: Option<u64>, pub total: Option<u64>, pub reasoning: Option<u64>, pub cached: Option<u64> }
#[derive(Debug, Clone, Default)] pub struct ResponseMetadata { pub id: Option<String>, pub model_id: Option<String>, pub timestamp: Option<i64>, pub headers: Option<http::HeaderMap>, pub body: Option<serde_json::Value> }
#[derive(Debug)] pub struct Message { pub role: u8, pub parts: Vec<serde_json::Value> }
#[derive(Debug, thiserror::Error)] #[error("provider error")] pub struct ProviderError { pub kind: u8, pub status: Option<StatusCode>, pub url: Option<Url>, pub message: String, pub body: Option<String>, pub headers: Option<http::HeaderMap>, pub retryable: bool }
#[derive(Debug, thiserror::Error)] #[error("mcp error")] pub struct McpError { pub code: i64, pub message: String, pub data: Option<serde_json::Value> }

/// Naive layout: fields inline as written in the design doc.
#[derive(Debug, thiserror::Error)]
pub enum ErrorInline {
    #[error(transparent)] Provider(#[from] ProviderError),
    #[error("retries exhausted")] Retry { reason: RetryReason, attempts: u32, errors: Vec<ProviderError> },
    #[error("timeout")] Timeout { scope: TimeoutScope, elapsed: Duration },
    #[error("cancelled")] Cancelled,
    #[error("invalid argument")] InvalidArgument { argument: &'static str, message: String },
    #[error("invalid prompt")] InvalidPrompt { message: String },
    #[error("message conversion")] MessageConversion { message: String, original_message: Box<Message> },
    #[error("download")] Download { url: Url, status_code: Option<StatusCode>, #[source] cause: Option<BoxError> },
    #[error("invalid data content")] InvalidDataContent { #[source] cause: Option<BoxError> },
    #[error("no such tool")] NoSuchTool { tool_name: ToolName, available_tools: Vec<ToolName> },
    #[error("invalid tool input")] InvalidToolInput { tool_name: ToolName, tool_input: String, #[source] cause: BoxError },
    #[error("repair")] ToolCallRepair { original: Box<ErrorInline>, #[source] cause: BoxError },
    #[error("tool choice")] ToolChoiceViolation { expected: ToolName, actual: ToolName },
    #[error("approval")] ToolCallNotFoundForApproval { tool_call_id: ToolCallId, approval_id: ApprovalId },
    #[error("approval")] InvalidToolApproval { approval_id: ApprovalId, message: String },
    #[error("no object")] NoObjectGenerated { text: Option<String>, response: ResponseMetadata, usage: Usage, finish_reason: FinishReason, #[source] cause: Option<BoxError> },
    #[error("no output")] NoOutputGenerated,
    #[error("no image")] NoImageGenerated { responses: Vec<ResponseMetadata> },
    #[error("no provider")] NoSuchProvider { provider_id: ProviderId, available_providers: Vec<ProviderId>, model_id: String, model_kind: ModelKind },
    #[error("no registry")] NoDefaultRegistry { model_id: String },
    #[error("stream part")] InvalidStreamPart { message: String },
    #[error(transparent)] Mcp(#[from] McpError),
    #[error(transparent)] Other(#[from] BoxError),
}

/// Boxed layout: every variant payload above 40 bytes moved behind a Box.
#[derive(Debug)] pub struct DownloadDetails { pub url: Url, pub status_code: Option<StatusCode>, pub cause: Option<BoxError> }
#[derive(Debug)] pub struct NoObjectDetails { pub text: Option<String>, pub response: ResponseMetadata, pub usage: Usage, pub finish_reason: FinishReason, pub cause: Option<BoxError> }
#[derive(Debug)] pub struct NoSuchProviderDetails { pub provider_id: ProviderId, pub available_providers: Vec<ProviderId>, pub model_id: String, pub model_kind: ModelKind }
#[derive(Debug)] pub struct InvalidToolInputDetails { pub tool_name: ToolName, pub tool_input: String, pub cause: BoxError }

#[derive(Debug, thiserror::Error)]
pub enum ErrorBoxed {
    #[error(transparent)] Provider(Box<ProviderError>),
    #[error("retries exhausted")] Retry { reason: RetryReason, attempts: u32, errors: Vec<ProviderError> },
    #[error("timeout")] Timeout { scope: TimeoutScope, elapsed: Duration },
    #[error("cancelled")] Cancelled,
    #[error("invalid argument")] InvalidArgument { argument: &'static str, message: String },
    #[error("invalid prompt")] InvalidPrompt { message: String },
    #[error("message conversion")] MessageConversion { message: String, original_message: Box<Message> },
    #[error("download")] Download(Box<DownloadDetails>),
    #[error("invalid data content")] InvalidDataContent { #[source] cause: Option<BoxError> },
    #[error("no such tool")] NoSuchTool { tool_name: ToolName, available_tools: Vec<ToolName> },
    #[error("invalid tool input")] InvalidToolInput(Box<InvalidToolInputDetails>),
    #[error("repair")] ToolCallRepair { original: Box<ErrorBoxed>, #[source] cause: BoxError },
    #[error("tool choice")] ToolChoiceViolation { expected: ToolName, actual: ToolName },
    #[error("approval")] ToolCallNotFoundForApproval { tool_call_id: ToolCallId, approval_id: ApprovalId },
    #[error("approval")] InvalidToolApproval { approval_id: ApprovalId, message: String },
    #[error("no object")] NoObjectGenerated(Box<NoObjectDetails>),
    #[error("no output")] NoOutputGenerated,
    #[error("no image")] NoImageGenerated { responses: Vec<ResponseMetadata> },
    #[error("no provider")] NoSuchProvider(Box<NoSuchProviderDetails>),
    #[error("no registry")] NoDefaultRegistry { model_id: String },
    #[error("stream part")] InvalidStreamPart { message: String },
    #[error(transparent)] Mcp(Box<McpError>),
    #[error(transparent)] Other(BoxError),
}

static_assertions::const_assert!(std::mem::size_of::<ErrorBoxed>() <= 128);

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn print_sizes() {
        println!("String={} Vec={} Box={} BoxError={} Option<BoxError>={} Url={} StatusCode={} Option<StatusCode>={} HeaderMap={} Value={} Duration={} ResponseMetadata={} Usage={} ProviderError={} McpError={}",
            size_of::<String>(), size_of::<Vec<u8>>(), size_of::<Box<u8>>(), size_of::<BoxError>(), size_of::<Option<BoxError>>(), size_of::<Url>(), size_of::<StatusCode>(), size_of::<Option<StatusCode>>(), size_of::<http::HeaderMap>(), size_of::<serde_json::Value>(), size_of::<Duration>(), size_of::<ResponseMetadata>(), size_of::<Usage>(), size_of::<ProviderError>(), size_of::<McpError>());
        println!("ErrorInline={} ErrorBoxed={} Result<(), ErrorBoxed>={}", size_of::<ErrorInline>(), size_of::<ErrorBoxed>(), size_of::<Result<(), ErrorBoxed>>());
        assert!(size_of::<ErrorBoxed>() <= 128);
        assert!(size_of::<ErrorInline>() > 128, "inline layout unexpectedly small: {}", size_of::<ErrorInline>());
    }
}
