//! Ferrin: a Rust AI SDK.
//!
//! This facade crate re-exports the public API of `ferrin-core` at the crate
//! root, the lower layers as modules ([`spec`], [`message`], [`schema`],
//! [`mod@tool`], [`provider_util`]) and, behind features, the first-party
//! provider crates ([`providers`]), the MCP client ([`mcp`]), the
//! OpenTelemetry bridge ([`otel`]) and policy-based tool approval
//! ([`policy`]). [`prelude`] gathers the items most programs need.
//!
//! Entry points are documented in `docs/02-api/02-api-reference.md`.
//!
//! # Examples
//!
//! ```
//! use ferrin::prelude::*;
//! use ferrin_testing::MockLanguageModel;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), ferrin::Error> {
//! let model = MockLanguageModel::builder()
//!     .generate(GenerateResult::new(vec![Content::text("hello")], FinishReason::stop()))
//!     .build_shared();
//! let result = generate_text(model).prompt("Say hello").await?;
//! assert_eq!(result.text(), "hello");
//! # Ok(())
//! # }
//! ```

pub use ferrin_spec::SPEC_VERSION;

// ---- lower layers as modules ----
pub use ferrin_message as message;
pub use ferrin_provider_util as provider_util;
pub use ferrin_schema as schema;
pub use ferrin_schema::schemars;
pub use ferrin_spec as spec;
pub use ferrin_tool as tool;
pub use serde;
pub use serde_json;

// ---- feature-gated extensions ----
#[cfg(feature = "macros")]
pub use ferrin_macros::tool;
#[cfg(feature = "mcp")]
pub use ferrin_mcp as mcp;
#[cfg(feature = "otel")]
pub use ferrin_otel as otel;
#[cfg(feature = "policy")]
pub use ferrin_policy as policy;

// ---- ferrin-core modules ----
pub use ferrin_core::agent;
pub use ferrin_core::batch;
pub use ferrin_core::clock;
pub use ferrin_core::embed;
pub use ferrin_core::error;
pub use ferrin_core::files;
pub use ferrin_core::generate_text;
pub use ferrin_core::hooks;
pub use ferrin_core::ids;
pub use ferrin_core::image;
pub use ferrin_core::middleware;
pub use ferrin_core::output;
pub use ferrin_core::prompt;
#[cfg(feature = "realtime")]
pub use ferrin_core::realtime;
pub use ferrin_core::registry;
pub use ferrin_core::rerank;
pub use ferrin_core::retry;
pub use ferrin_core::skills;
pub use ferrin_core::speech;
pub use ferrin_core::speech_translation;
pub use ferrin_core::stream_text;
pub use ferrin_core::telemetry;
pub use ferrin_core::timeout;
pub use ferrin_core::transcription;
pub use ferrin_core::video;

// ---- ferrin-core entry points and core types ----
pub use ferrin_core::Agent;
pub use ferrin_core::AgentCall;
pub use ferrin_core::CallSettings;
pub use ferrin_core::Clock;
pub use ferrin_core::EmbeddingModelMiddleware;
pub use ferrin_core::Error;
pub use ferrin_core::ErrorKind;
pub use ferrin_core::GenerateText;
pub use ferrin_core::GenerateTextResult;
pub use ferrin_core::HookFn;
pub use ferrin_core::Hooks;
pub use ferrin_core::ImageModelMiddleware;
pub use ferrin_core::Instructions;
pub use ferrin_core::LanguageModelMiddleware;
pub use ferrin_core::Output;
pub use ferrin_core::ProviderMiddleware;
pub use ferrin_core::ProviderRegistry;
#[cfg(feature = "realtime")]
pub use ferrin_core::RealtimeSession;
pub use ferrin_core::RetryPolicy;
pub use ferrin_core::StepContent;
pub use ferrin_core::StepResult;
pub use ferrin_core::StreamEvent;
pub use ferrin_core::StreamText;
pub use ferrin_core::StreamTextResult;
pub use ferrin_core::Telemetry;
pub use ferrin_core::TelemetryOptions;
pub use ferrin_core::Timeout;
pub use ferrin_core::ToolLoopAgent;
pub use ferrin_core::cancel_batch;
pub use ferrin_core::cosine_similarity;
pub use ferrin_core::create_provider_registry;
pub use ferrin_core::custom_provider;
pub use ferrin_core::embed_many;
pub use ferrin_core::generate_image;
pub use ferrin_core::generate_speech;
pub use ferrin_core::generate_video;
pub use ferrin_core::get_batch_results;
pub use ferrin_core::get_batch_status;
pub use ferrin_core::has_tool_call;
pub use ferrin_core::list_batches;
#[cfg(feature = "realtime")]
pub use ferrin_core::realtime_session;
pub use ferrin_core::start_batch;
pub use ferrin_core::step_count;
pub use ferrin_core::stream_speech_translation;
pub use ferrin_core::stream_transcribe;
pub use ferrin_core::transcribe;
pub use ferrin_core::upload_file;
pub use ferrin_core::upload_skill;
pub use ferrin_core::wrap_embedding_model;
pub use ferrin_core::wrap_image_model;
pub use ferrin_core::wrap_language_model;
pub use ferrin_core::wrap_provider;

/// First-party provider crates, enabled by the feature of the same name.
///
/// Each provider is also available at the crate root (for example
/// `ferrin::openai`).
pub mod providers {
    #[cfg(feature = "anthropic")]
    pub use ferrin_anthropic as anthropic;
    #[cfg(feature = "google")]
    pub use ferrin_google as google;
    #[cfg(feature = "openai")]
    pub use ferrin_openai as openai;
    #[cfg(feature = "openai-compatible")]
    pub use ferrin_openai_compatible as openai_compatible;
}

#[cfg(feature = "anthropic")]
pub use ferrin_anthropic as anthropic;
#[cfg(feature = "google")]
pub use ferrin_google as google;
#[cfg(feature = "openai")]
pub use ferrin_openai as openai;
#[cfg(feature = "openai-compatible")]
pub use ferrin_openai_compatible as openai_compatible;

/// The items most programs need: entry points, result types, messages,
/// tools, common specification types, the serde derives and `json!`, and
/// `StreamExt` for consuming streams.
///
/// The serde and schemars derives expand to paths in those crates; either
/// depend on `serde`/`schemars` directly or add
/// `#[serde(crate = "ferrin::serde")]` and
/// `#[schemars(crate = "ferrin::schemars")]` to the type.
pub mod prelude {
    pub use ferrin_core::Agent;
    pub use ferrin_core::AgentCall;
    pub use ferrin_core::CallSettings;
    pub use ferrin_core::EmbeddingModelMiddleware;
    pub use ferrin_core::Error;
    pub use ferrin_core::ErrorKind;
    pub use ferrin_core::GenerateText;
    pub use ferrin_core::GenerateTextResult;
    pub use ferrin_core::ImageModelMiddleware;
    pub use ferrin_core::Instructions;
    pub use ferrin_core::LanguageModelMiddleware;
    pub use ferrin_core::Output;
    pub use ferrin_core::ProviderMiddleware;
    pub use ferrin_core::ProviderRegistry;
    #[cfg(feature = "realtime")]
    pub use ferrin_core::RealtimeSession;
    pub use ferrin_core::RetryPolicy;
    pub use ferrin_core::StepContent;
    pub use ferrin_core::StepResult;
    pub use ferrin_core::StreamEvent;
    pub use ferrin_core::StreamText;
    pub use ferrin_core::StreamTextResult;
    pub use ferrin_core::Telemetry;
    pub use ferrin_core::TelemetryOptions;
    pub use ferrin_core::Timeout;
    pub use ferrin_core::ToolLoopAgent;
    pub use ferrin_core::cosine_similarity;
    pub use ferrin_core::create_provider_registry;
    pub use ferrin_core::embed;
    pub use ferrin_core::embed_many;
    pub use ferrin_core::generate_image;
    pub use ferrin_core::generate_speech;
    pub use ferrin_core::generate_text;
    pub use ferrin_core::has_tool_call;
    #[cfg(feature = "realtime")]
    pub use ferrin_core::realtime_session;
    pub use ferrin_core::rerank;
    pub use ferrin_core::step_count;
    pub use ferrin_core::stream_text;
    pub use ferrin_core::transcribe;
    pub use ferrin_core::upload_file;
    pub use ferrin_core::wrap_embedding_model;
    pub use ferrin_core::wrap_image_model;
    pub use ferrin_core::wrap_language_model;
    pub use ferrin_core::wrap_provider;
    #[cfg(feature = "macros")]
    pub use ferrin_macros::tool;
    pub use ferrin_message::AssistantPart;
    pub use ferrin_message::Message;
    pub use ferrin_message::MessagesExt;
    pub use ferrin_message::Role;
    pub use ferrin_message::ToolApprovalResponse;
    pub use ferrin_message::UserPart;
    pub use ferrin_spec::Content;
    pub use ferrin_spec::EmbeddingModel;
    pub use ferrin_spec::FinishReason;
    pub use ferrin_spec::FinishReasonKind;
    pub use ferrin_spec::GenerateResult;
    pub use ferrin_spec::Headers;
    pub use ferrin_spec::ImageModel;
    pub use ferrin_spec::ImageSize;
    pub use ferrin_spec::JsonObject;
    pub use ferrin_spec::JsonValue;
    pub use ferrin_spec::LanguageModel;
    pub use ferrin_spec::LanguageModelRef;
    pub use ferrin_spec::ProviderError;
    pub use ferrin_spec::ProviderOptions;
    pub use ferrin_spec::ReasoningEffort;
    pub use ferrin_spec::StreamPart;
    pub use ferrin_spec::ToolChoice;
    pub use ferrin_spec::Usage;
    pub use ferrin_tool::JsonSchema;
    pub use ferrin_tool::NeedsApproval;
    pub use ferrin_tool::Schema;
    pub use ferrin_tool::Tool;
    pub use ferrin_tool::ToolContext;
    pub use ferrin_tool::ToolError;
    pub use ferrin_tool::ToolSet;
    pub use futures_util::StreamExt;
    pub use serde::Deserialize;
    pub use serde::Serialize;
    pub use serde_json::json;
}
