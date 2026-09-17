//! Ferrin core.
//!
//! Text generation loop, streaming pipeline, structured output, agents,
//! middleware, provider registry, retry and timeout policies, and the other
//! modalities (embeddings, images, speech, transcription, reranking, video).
//!
//! Design: `docs/01-architecture/07-generation-loop-and-streaming.md` and following.
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the `NOTICE` file in the crate root.

pub mod agent;
pub mod batch;
mod builder;
pub(crate) mod cancel;
pub mod clock;
pub mod embed;
pub mod error;
pub mod files;
pub mod generate_text;
pub mod hooks;
pub mod ids;
pub mod image;
pub(crate) mod limits;
pub mod middleware;
pub(crate) mod modality;
mod modality_hooks;
mod modality_metadata;
mod modality_stream;
pub mod output;
pub mod prompt;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod registry;
pub mod rerank;
pub mod retry;
pub mod skills;
pub mod speech;
pub mod speech_translation;
pub mod stream_text;
pub mod telemetry;
pub mod timeout;
pub mod transcription;
#[cfg(feature = "video")]
pub mod video;

pub use agent::Agent;
pub use agent::AgentCall;
pub use agent::ToolLoopAgent;
pub use batch::cancel_batch;
pub use batch::get_batch_results;
pub use batch::get_batch_status;
pub use batch::list_batches;
pub use batch::start_batch;
pub use clock::Clock;
pub use embed::cosine_similarity;
pub use embed::embed;
pub use embed::embed_many;
pub use error::Error;
pub use error::ErrorKind;
pub use files::upload_file;
pub use generate_text::GenerateText;
pub use generate_text::GenerateTextResult;
pub use generate_text::StepContent;
pub use generate_text::StepResult;
pub use generate_text::generate_text;
pub use generate_text::has_tool_call;
pub use generate_text::step_count;
pub use hooks::HookFn;
pub use hooks::Hooks;
pub use image::generate_image;
pub use middleware::EmbeddingModelMiddleware;
pub use middleware::ImageModelMiddleware;
pub use middleware::LanguageModelMiddleware;
pub use middleware::ProviderMiddleware;
pub use middleware::wrap_embedding_model;
pub use middleware::wrap_image_model;
pub use middleware::wrap_language_model;
pub use middleware::wrap_provider;
pub use output::Output;
pub use prompt::CallSettings;
pub use prompt::Instructions;
#[cfg(feature = "realtime")]
pub use realtime::RealtimeSession;
#[cfg(feature = "realtime")]
pub use realtime::realtime_session;
pub use registry::ProviderRegistry;
pub use registry::create_provider_registry;
pub use registry::custom_provider;
pub use rerank::rerank;
pub use retry::RetryPolicy;
pub use skills::upload_skill;
pub use speech::generate_speech;
pub use speech_translation::stream_speech_translation;
pub use stream_text::StreamEvent;
pub use stream_text::StreamText;
pub use stream_text::StreamTextResult;
pub use stream_text::stream_text;
pub use telemetry::Telemetry;
pub use telemetry::TelemetryOptions;
pub use timeout::Timeout;
pub use transcription::stream_transcribe;
pub use transcription::transcribe;
#[cfg(feature = "video")]
pub use video::generate_video;

/// User-agent suffix appended to every model request.
pub(crate) const USER_AGENT: &str = concat!("ferrin/", env!("CARGO_PKG_VERSION"));
