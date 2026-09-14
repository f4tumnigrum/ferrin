//! Language model interface: call options, prompt, tools, content, stream
//! parts, results and the [`LanguageModel`] trait.

use std::future::Future;

use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;

pub mod call_options;
pub mod content;
pub mod finish_reason;
pub mod prompt;
pub mod result;
pub mod stream_part;
pub mod supported_urls;
pub mod tool;
pub mod usage;

pub use call_options::CallOptions;
pub use call_options::CallOptionsRecord;
pub use call_options::ReasoningEffort;
pub use call_options::ResponseFormat;
pub use call_options::ToolChoice;
pub use content::Content;
pub use content::CustomKind;
pub use content::InvalidCustomKind;
pub use content::ProviderToolResult;
pub use content::Source;
pub use content::ToolCall;
pub use finish_reason::FinishReason;
pub use finish_reason::FinishReasonKind;
pub use prompt::Prompt;
pub use prompt::PromptMessage;
pub use result::GenerateResult;
pub use result::RequestMetadata;
pub use result::ResponseMetadata;
pub use result::StreamResult;
pub use stream_part::StreamError;
pub use stream_part::StreamErrorCode;
pub use stream_part::StreamPart;
pub use supported_urls::SupportedUrls;
pub use tool::ToolDefinition;
pub use usage::InputTokens;
pub use usage::OutputTokens;
pub use usage::Usage;
pub use usage::add_token_counts;

/// A text generation model.
///
/// Implement this trait in a provider crate for every language model API.
/// Implementations must be cheap to clone behind an `Arc` and safe to call
/// concurrently. See the adapter contract in the provider specification:
/// unsupported options produce warnings, tool input is passed through as raw
/// JSON text, streams start with `StreamStart` and end with `Finish` or
/// `Error`, and the cancellation token aborts the HTTP request.
pub trait LanguageModel: Send + Sync + 'static {
    /// Provider identifier, for example `openai.responses`.
    fn provider(&self) -> &ProviderId;

    /// Model identifier as sent to the provider.
    fn model_id(&self) -> &ModelId;

    /// URL patterns (by media type) the provider can fetch itself.
    ///
    /// Files whose URL does not match are downloaded by the core and inlined.
    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send;

    /// Generates a complete response.
    fn do_generate(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<GenerateResult, ProviderError>> + Send;

    /// Generates a streamed response.
    fn do_stream(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<StreamResult, ProviderError>> + Send;
}
