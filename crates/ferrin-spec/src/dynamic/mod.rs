//! Object-safe adapters (`Dyn*` traits) and reference types.
//!
//! Every model trait uses `impl Future` return types and is therefore not
//! object safe. For each trait this module provides a `Dyn*` counterpart that
//! boxes the futures, a blanket implementation so that every concrete model
//! is also a `Dyn*`, and a cloneable reference type (`*Ref`) wrapping
//! `Arc<dyn Dyn*>`.
//!
//! Only import a `Dyn*` trait where you hold a trait object; importing both
//! `LanguageModel` and `DynLanguageModel` makes method calls on a concrete
//! model ambiguous.

use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;

mod batch;
mod embedding_model;
mod files;
mod image_model;
mod language_model;
mod model_ref;
mod realtime_model;
mod reranking_model;
mod skills;
mod speech_model;
mod speech_translation_model;
mod transcription_model;
mod video_model;

pub use batch::BatchRef;
pub use batch::DynBatch;
pub use embedding_model::DynEmbeddingModel;
pub use embedding_model::EmbeddingModelRef;
pub use files::DynFiles;
pub use files::FilesRef;
pub use image_model::DynImageModel;
pub use image_model::ImageModelRef;
pub use language_model::DynLanguageModel;
pub use language_model::LanguageModelRef;
pub use model_ref::ModelRef;
pub use model_ref::ServiceRef;
pub use realtime_model::DynRealtimeFactory;
pub use realtime_model::DynRealtimeModel;
pub use realtime_model::RealtimeFactoryRef;
pub use realtime_model::RealtimeModelRef;
pub use reranking_model::DynRerankingModel;
pub use reranking_model::RerankingModelRef;
pub use skills::DynSkills;
pub use skills::SkillsRef;
pub use speech_model::DynSpeechModel;
pub use speech_model::SpeechModelRef;
pub use speech_translation_model::DynSpeechTranslationModel;
pub use speech_translation_model::SpeechTranslationModelRef;
pub use transcription_model::DynTranscriptionModel;
pub use transcription_model::TranscriptionModelRef;
pub use video_model::DynVideoModel;
pub use video_model::VideoModelRef;

/// A boxed, `Send` future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed, `Send` stream.
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;
