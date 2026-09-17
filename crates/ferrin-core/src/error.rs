//! Core error type: the single `#[non_exhaustive]` enum applications see.
//!
//! Payloads that would push the enum past 128 bytes are boxed
//! (`Provider`, `Download`, `InvalidToolInput`, `NoObjectGenerated`,
//! `NoSuchProvider`); see the error-model design document.

use std::time::Duration;

use ferrin_message::Message;
use ferrin_spec::ApprovalId;
use ferrin_spec::FinishReason;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::StreamError;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use crate::timeout::TimeoutScope;

/// A boxed error.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Errors returned by the core API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A provider adapter failed.
    #[error(transparent)]
    Provider(Box<ProviderError>),

    /// Retries were exhausted or stopped early.
    #[error("retries exhausted after {attempts} attempts ({reason})")]
    Retry {
        /// Why retrying stopped.
        reason: RetryReason,
        /// Number of attempts made.
        attempts: u32,
        /// Errors of every attempt, oldest first.
        errors: Vec<ProviderError>,
    },

    /// A configured timeout elapsed.
    #[error("timeout ({scope}) after {elapsed:?}")]
    Timeout {
        /// Which timeout fired.
        scope: TimeoutScope,
        /// Time elapsed when it fired.
        elapsed: Duration,
    },

    /// The caller cancelled the operation.
    #[error("operation cancelled")]
    Cancelled,

    /// A call setting is invalid.
    #[error("invalid argument `{argument}`: {message}")]
    InvalidArgument {
        /// Name of the offending argument.
        argument: String,
        /// What is wrong with it.
        message: String,
    },

    /// The prompt is invalid (empty, both `prompt` and `messages`, ...).
    #[error("invalid prompt: {message}")]
    InvalidPrompt {
        /// What is wrong with it.
        message: String,
    },

    /// A message could not be converted to the specification prompt.
    #[error("message conversion failed: {message}")]
    MessageConversion {
        /// What is wrong.
        message: String,
        /// The message concerned.
        original_message: Box<Message>,
    },

    /// A URL in the prompt could not be downloaded.
    #[error("download failed for {}", .0.url)]
    Download(#[source] Box<DownloadDetails>),

    /// Inline data content (base64, data URL) is invalid.
    #[error("invalid data content: {message}")]
    InvalidDataContent {
        /// What is wrong.
        message: String,
        /// Underlying cause.
        #[source]
        cause: Option<BoxError>,
    },

    /// The model called a tool that is not in the tool set.
    #[error("no such tool `{tool_name}`")]
    NoSuchTool {
        /// The unknown name.
        tool_name: ToolName,
        /// Names the model could have used.
        available_tools: Vec<ToolName>,
    },

    /// The model produced input that does not match the tool schema.
    #[error("invalid input for tool `{}`", .0.tool_name)]
    InvalidToolInput(#[source] Box<InvalidToolInputDetails>),

    /// The tool call repair function failed.
    #[error("tool call repair failed")]
    ToolCallRepair {
        /// The error the repair function was asked to fix.
        original: Box<Error>,
        /// The repair function's own error.
        #[source]
        cause: BoxError,
    },

    /// `tool_choice` required one tool but the model called another.
    #[error("tool choice violated: expected `{expected}`, got `{actual}`")]
    ToolChoiceViolation {
        /// The required tool.
        expected: ToolName,
        /// The tool actually called.
        actual: ToolName,
    },

    /// An approval response refers to a tool call missing from the history.
    #[error("tool call `{tool_call_id}` not found for approval `{approval_id}`")]
    ToolCallNotFoundForApproval {
        /// The referenced tool call.
        tool_call_id: ToolCallId,
        /// The approval concerned.
        approval_id: ApprovalId,
    },

    /// The model finished without the tool call that the tool choice
    /// required.
    #[error("tool choice not satisfied: {}", .expected.as_ref().map_or_else(|| "a tool call was required".to_owned(), |name| format!("expected a call to `{name}`")))]
    ToolChoiceNotSatisfied {
        /// The required tool, when the choice named one.
        expected: Option<ToolName>,
    },

    /// An approval response is invalid (unknown request, bad signature).
    #[error("invalid tool approval `{approval_id}`: {message}")]
    InvalidToolApproval {
        /// The approval concerned.
        approval_id: ApprovalId,
        /// What is wrong.
        message: String,
    },

    /// Structured output could not be parsed or validated.
    #[error("no structured output generated: {}", .0.message)]
    NoObjectGenerated(#[source] Box<NoObjectGeneratedDetails>),

    /// The last step did not qualify for structured output parsing.
    #[error("no output generated")]
    NoOutputGenerated,

    /// Every image call returned no image.
    #[error("no image generated")]
    NoImageGenerated {
        /// Responses of the attempted calls.
        responses: Vec<ResponseMetadata>,
    },

    /// The speech model returned no audio.
    #[error("no speech generated")]
    NoSpeechGenerated {
        /// Responses of the attempted calls.
        responses: Vec<ResponseMetadata>,
    },

    /// The transcription model returned no text.
    #[error("no transcript generated")]
    NoTranscriptGenerated {
        /// Responses of the attempted calls.
        responses: Vec<ResponseMetadata>,
    },

    /// The speech translation stream produced no translated text or audio.
    #[error("no translation generated")]
    NoTranslationGenerated {
        /// Response metadata accumulated before the stream ended.
        response: Box<ResponseMetadata>,
    },

    /// Every video call returned no video.
    #[error("no video generated")]
    NoVideoGenerated {
        /// Responses of the attempted calls.
        responses: Vec<ResponseMetadata>,
    },

    /// The registry has no provider with the requested id.
    #[error("no such provider `{}`", .0.provider_id)]
    NoSuchProvider(Box<NoSuchProviderDetails>),

    /// A `provider:model` string was used without a default registry.
    #[error("no default registry configured for model id `{model_id}`")]
    NoDefaultRegistry {
        /// The unresolved id.
        model_id: String,
    },

    /// The provider stream violated the specification contract.
    #[error("invalid stream part: {message}")]
    InvalidStreamPart {
        /// What is wrong.
        message: String,
    },

    /// The provider stream reported an error part.
    #[error("stream error: {}", .0.message)]
    Stream(Box<StreamError>),

    /// An MCP error (type-erased: the core does not depend on the MCP crate).
    #[error(transparent)]
    Mcp(BoxError),

    /// Any other error.
    #[error(transparent)]
    Other(BoxError),
}

/// Why a retry loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetryReason {
    /// The last attempt failed and no retries were left.
    MaxRetriesExceeded,
    /// A non-retryable error occurred after at least one retry.
    ErrorNotRetryable,
    /// The cancellation token fired while waiting to retry.
    Abort,
}

impl std::fmt::Display for RetryReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MaxRetriesExceeded => "max retries exceeded",
            Self::ErrorNotRetryable => "error not retryable",
            Self::Abort => "aborted",
        })
    }
}

/// Payload of [`Error::Download`].
#[derive(Debug, thiserror::Error)]
#[error("download of {url} failed")]
pub struct DownloadDetails {
    /// The URL.
    pub url: Url,
    /// HTTP status when the server answered.
    pub status_code: Option<StatusCode>,
    /// Underlying cause.
    #[source]
    pub cause: Option<BoxError>,
}

/// Payload of [`Error::InvalidToolInput`].
#[derive(Debug, thiserror::Error)]
#[error("invalid input for tool `{tool_name}`: {tool_input}")]
pub struct InvalidToolInputDetails {
    /// The tool.
    pub tool_name: ToolName,
    /// The raw input text.
    pub tool_input: String,
    /// The parse or validation error.
    #[source]
    pub cause: BoxError,
}

/// Payload of [`Error::NoObjectGenerated`].
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct NoObjectGeneratedDetails {
    /// Why parsing failed.
    pub message: String,
    /// The text the model produced, if any.
    pub text: Option<String>,
    /// Response metadata of the last step.
    pub response: ResponseMetadata,
    /// Usage of the last step.
    pub usage: Usage,
    /// Finish reason of the last step.
    pub finish_reason: FinishReason,
    /// Underlying cause.
    #[source]
    pub cause: Option<BoxError>,
}

/// Payload of [`Error::NoSuchProvider`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoSuchProviderDetails {
    /// The requested provider id.
    pub provider_id: ProviderId,
    /// Providers the registry knows.
    pub available_providers: Vec<ProviderId>,
    /// The full model id that was requested.
    pub model_id: String,
    /// The kind of model requested.
    pub model_kind: ModelKind,
}

/// Low-cardinality error category for logs and telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ErrorKind {
    /// Provider adapter failure.
    Provider,
    /// Retries exhausted.
    Retry,
    /// Timeout.
    Timeout,
    /// Cancelled.
    Cancelled,
    /// Invalid input (arguments, prompt, data content, stream contract).
    InvalidInput,
    /// Tool-related failure (unknown tool, invalid input, approval).
    Tool,
    /// Output-related failure (no object/output/image/...).
    Output,
    /// Missing provider or registry.
    NotFound,
    /// MCP failure.
    Mcp,
    /// Anything else.
    Other,
}

impl ErrorKind {
    /// Returns the kebab-case name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Retry => "retry",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::InvalidInput => "invalid-input",
            Self::Tool => "tool",
            Self::Output => "output",
            Self::NotFound => "not-found",
            Self::Mcp => "mcp",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Error {
    /// Creates an [`Error::InvalidArgument`].
    #[must_use]
    pub fn invalid_argument(argument: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            argument: argument.into(),
            message: message.into(),
        }
    }

    /// Creates an [`Error::InvalidPrompt`].
    #[must_use]
    pub fn invalid_prompt(message: impl Into<String>) -> Self {
        Self::InvalidPrompt {
            message: message.into(),
        }
    }

    /// Creates an [`Error::InvalidDataContent`].
    #[must_use]
    pub fn invalid_data_content(message: impl Into<String>, cause: Option<BoxError>) -> Self {
        Self::InvalidDataContent {
            message: message.into(),
            cause,
        }
    }

    /// Creates an [`Error::Download`].
    #[must_use]
    pub fn download(url: Url, status_code: Option<StatusCode>, cause: Option<BoxError>) -> Self {
        Self::Download(Box::new(DownloadDetails {
            url,
            status_code,
            cause,
        }))
    }

    /// Creates an [`Error::NoSuchTool`].
    #[must_use]
    pub fn no_such_tool(tool_name: impl Into<ToolName>, available_tools: Vec<ToolName>) -> Self {
        Self::NoSuchTool {
            tool_name: tool_name.into(),
            available_tools,
        }
    }

    /// Creates an [`Error::InvalidToolInput`].
    #[must_use]
    pub fn invalid_tool_input(
        tool_name: impl Into<ToolName>,
        tool_input: impl Into<String>,
        cause: impl Into<BoxError>,
    ) -> Self {
        Self::InvalidToolInput(Box::new(InvalidToolInputDetails {
            tool_name: tool_name.into(),
            tool_input: tool_input.into(),
            cause: cause.into(),
        }))
    }

    /// Creates an [`Error::NoObjectGenerated`].
    #[must_use]
    pub fn no_object_generated(details: NoObjectGeneratedDetails) -> Self {
        Self::NoObjectGenerated(Box::new(details))
    }

    /// Creates an [`Error::NoSuchProvider`].
    #[must_use]
    pub fn no_such_provider(details: NoSuchProviderDetails) -> Self {
        Self::NoSuchProvider(Box::new(details))
    }

    /// Creates an [`Error::InvalidStreamPart`].
    #[must_use]
    pub fn invalid_stream_part(message: impl Into<String>) -> Self {
        Self::InvalidStreamPart {
            message: message.into(),
        }
    }

    /// Wraps a stream error part as [`Error::Stream`].
    #[must_use]
    pub fn stream(error: StreamError) -> Self {
        Self::Stream(Box::new(error))
    }

    /// Wraps any error as [`Error::Other`].
    #[must_use]
    pub fn other(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other(Box::new(error))
    }

    /// Wraps a message as [`Error::Other`].
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Other(message.into().into())
    }

    /// Returns `true` when retrying the operation may succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Provider(error) => error.is_retryable(),
            Self::Retry {
                reason: RetryReason::MaxRetriesExceeded,
                errors,
                ..
            } => errors.last().is_some_and(ProviderError::is_retryable),
            Self::Stream(error) => error.is_retryable.unwrap_or(false),
            _ => false,
        }
    }

    /// Returns `true` when the error is the result of cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Retry {
                    reason: RetryReason::Abort,
                    ..
                }
        )
    }

    /// Returns the HTTP status of the underlying provider response, if any.
    #[must_use]
    pub fn status_code(&self) -> Option<StatusCode> {
        match self {
            Self::Provider(error) => error.status_code(),
            Self::Retry { errors, .. } => errors.last().and_then(ProviderError::status_code),
            Self::Download(details) => details.status_code,
            Self::Stream(error) => error
                .status_code
                .and_then(|code| StatusCode::from_u16(code).ok()),
            _ => None,
        }
    }

    /// Returns the provider error when this error wraps one.
    #[must_use]
    pub fn as_provider(&self) -> Option<&ProviderError> {
        match self {
            Self::Provider(error) => Some(error),
            _ => None,
        }
    }

    /// Returns the coarse category.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Provider(_) | Self::Stream(_) => ErrorKind::Provider,
            Self::Retry { .. } => ErrorKind::Retry,
            Self::Timeout { .. } => ErrorKind::Timeout,
            Self::Cancelled => ErrorKind::Cancelled,
            Self::InvalidArgument { .. }
            | Self::InvalidPrompt { .. }
            | Self::MessageConversion { .. }
            | Self::Download(_)
            | Self::InvalidDataContent { .. }
            | Self::InvalidStreamPart { .. } => ErrorKind::InvalidInput,
            Self::NoSuchTool { .. }
            | Self::InvalidToolInput(_)
            | Self::ToolCallRepair { .. }
            | Self::ToolChoiceViolation { .. }
            | Self::ToolChoiceNotSatisfied { .. }
            | Self::ToolCallNotFoundForApproval { .. }
            | Self::InvalidToolApproval { .. } => ErrorKind::Tool,
            Self::NoObjectGenerated(_)
            | Self::NoOutputGenerated
            | Self::NoImageGenerated { .. }
            | Self::NoSpeechGenerated { .. }
            | Self::NoTranscriptGenerated { .. }
            | Self::NoTranslationGenerated { .. }
            | Self::NoVideoGenerated { .. } => ErrorKind::Output,
            Self::NoSuchProvider(_) | Self::NoDefaultRegistry { .. } => ErrorKind::NotFound,
            Self::Mcp(_) => ErrorKind::Mcp,
            Self::Other(_) => ErrorKind::Other,
        }
    }
}

impl From<ProviderError> for Error {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Cancelled => Self::Cancelled,
            other => Self::Provider(Box::new(other)),
        }
    }
}

impl From<ferrin_spec::error::NoSuchModelError> for Error {
    fn from(error: ferrin_spec::error::NoSuchModelError) -> Self {
        Self::Provider(Box::new(ProviderError::NoSuchModel(Box::new(error))))
    }
}

impl From<ferrin_spec::error::InvalidArgumentError> for Error {
    fn from(error: ferrin_spec::error::InvalidArgumentError) -> Self {
        Self::InvalidArgument {
            argument: error.argument,
            message: error.message,
        }
    }
}

impl From<ferrin_tool::ToolError> for Error {
    fn from(error: ferrin_tool::ToolError) -> Self {
        match error {
            ferrin_tool::ToolError::Cancelled => Self::Cancelled,
            other => Self::Other(Box::new(other)),
        }
    }
}
