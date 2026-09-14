//! Ferrin provider specification.
//!
//! This crate defines the contract between the Ferrin core and provider
//! adapters: model traits, prompt and content types, stream parts, shared
//! option and metadata types, and the provider-level error type. It contains no
//! networking and no generation logic.
//!
//! # Layout
//!
//! - [`shared`]: identifiers, provider options and metadata, warnings, headers,
//!   media types, file data.
//! - [`language_model`]: the [`LanguageModel`] trait and everything it exchanges
//!   with the core (call options, prompt, tools, content, stream parts, results).
//! - One module per additional model kind ([`embedding_model`], [`image_model`],
//!   [`speech_model`], [`transcription_model`], [`reranking_model`],
//!   [`video_model`], [`speech_translation_model`], [`realtime_model`]) and per
//!   provider service ([`files`], [`skills`], [`batch`]).
//! - [`provider`]: the [`Provider`] trait that hands out models by id.
//! - [`dynamic`]: object-safe `Dyn*` traits, boxed future/stream aliases and
//!   the cloneable `*Ref` handle types.
//! - [`error`]: [`ProviderError`] and its concrete error structs.
//!
//! # Serialization
//!
//! Every wire type derives `serde` with internally tagged enums (`type` or
//! `role`, kebab-case), snake_case fields, base64 for bytes and RFC 3339 for
//! timestamps. Optional fields are omitted when absent. `serde_json` is used
//! with `preserve_order`, so object key order is stable.
//!
//! Design: `docs/01-architecture/03-core-data-model.md` and
//! `docs/01-architecture/04-provider-spec.md`.

pub mod batch;
pub mod dynamic;
pub mod embedding_model;
pub mod error;
pub mod files;
pub mod image_model;
pub mod json;
pub mod language_model;
pub mod provider;
pub mod realtime_model;
pub mod reranking_model;
pub mod shared;
pub mod skills;
pub mod speech_model;
pub mod speech_translation_model;
pub mod transcription_model;
pub mod video_model;

pub use batch::Batch;
pub use dynamic::BatchRef;
pub use dynamic::BoxFuture;
pub use dynamic::BoxStream;
pub use dynamic::DynBatch;
pub use dynamic::DynEmbeddingModel;
pub use dynamic::DynFiles;
pub use dynamic::DynImageModel;
pub use dynamic::DynLanguageModel;
pub use dynamic::DynRealtimeFactory;
pub use dynamic::DynRealtimeModel;
pub use dynamic::DynRerankingModel;
pub use dynamic::DynSkills;
pub use dynamic::DynSpeechModel;
pub use dynamic::DynSpeechTranslationModel;
pub use dynamic::DynTranscriptionModel;
pub use dynamic::DynVideoModel;
pub use dynamic::EmbeddingModelRef;
pub use dynamic::FilesRef;
pub use dynamic::ImageModelRef;
pub use dynamic::LanguageModelRef;
pub use dynamic::ModelRef;
pub use dynamic::RealtimeFactoryRef;
pub use dynamic::RealtimeModelRef;
pub use dynamic::RerankingModelRef;
pub use dynamic::ServiceRef;
pub use dynamic::SkillsRef;
pub use dynamic::SpeechModelRef;
pub use dynamic::SpeechTranslationModelRef;
pub use dynamic::TranscriptionModelRef;
pub use dynamic::VideoModelRef;
pub use embedding_model::EmbeddingModel;
pub use error::ApiCallError;
pub use error::ModelKind;
pub use error::NoSuchModelError;
pub use error::ProviderError;
pub use error::UnsupportedFunctionalityError;
pub use files::Files;
pub use image_model::AspectRatio;
pub use image_model::ImageModel;
pub use image_model::ImageSize;
pub use json::JsonObject;
pub use json::JsonValue;
pub use language_model::CallOptions;
pub use language_model::Content;
pub use language_model::CustomKind;
pub use language_model::FinishReason;
pub use language_model::FinishReasonKind;
pub use language_model::GenerateResult;
pub use language_model::LanguageModel;
pub use language_model::Prompt;
pub use language_model::PromptMessage;
pub use language_model::ReasoningEffort;
pub use language_model::RequestMetadata;
pub use language_model::ResponseFormat;
pub use language_model::ResponseMetadata;
pub use language_model::StreamError;
pub use language_model::StreamPart;
pub use language_model::StreamResult;
pub use language_model::SupportedUrls;
pub use language_model::ToolCall;
pub use language_model::ToolChoice;
pub use language_model::ToolDefinition;
pub use language_model::Usage;
pub use provider::Provider;
pub use provider::ProviderRef;
pub use realtime_model::RealtimeFactory;
pub use realtime_model::RealtimeModel;
pub use reranking_model::RerankingModel;
pub use shared::ApprovalId;
pub use shared::AudioFormat;
pub use shared::BatchId;
pub use shared::FileData;
pub use shared::Headers;
pub use shared::InvalidHeader;
pub use shared::MediaType;
pub use shared::ModelId;
pub use shared::PartId;
pub use shared::ProviderId;
pub use shared::ProviderMetadata;
pub use shared::ProviderOptions;
pub use shared::ProviderReference;
pub use shared::ToolCallId;
pub use shared::ToolName;
pub use shared::Warning;
pub use skills::Skills;
pub use speech_model::SpeechModel;
pub use speech_translation_model::SpeechTranslationModel;
pub use transcription_model::TranscriptionModel;
pub use video_model::VideoModel;

/// Version of the provider specification implemented by this crate.
///
/// The specification version equals the crate version (ADR 0011). Adapters
/// compiled against a different `ferrin-spec` version fail to link rather than
/// negotiating at runtime.
pub const SPEC_VERSION: &str = env!("CARGO_PKG_VERSION");
