//! Ferrin application-level messages.
//!
//! - [`Message`] and its role structs: the conversation model applications
//!   build and persist. Content may be a plain string or a list of parts.
//! - Part types: [`UserPart`], [`AssistantPart`], [`ToolPart`] and the
//!   application-only parts [`ImagePart`], [`FilePart`],
//!   [`ReasoningFilePart`], [`ToolApprovalRequest`],
//!   [`ToolApprovalResponse`]. Parts whose shape equals the provider
//!   specification (`TextPart`, `ReasoningPart`, `CustomPart`,
//!   `ToolCallPart`, `ToolResultPart`, `ToolResultOutput`) are re-exported
//!   from `ferrin_spec`.
//! - [`FileSource`]: application file payloads (bytes, base64, URL, provider
//!   reference, inline text, local path) plus [`data_url`] parsing.
//! - [`prune`]: removal of reasoning, tool calls and empty messages from a
//!   history.
//!
//! Conversion to the provider `Prompt` (downloads, media type detection)
//! lives in the core crate.
//!
//! Design: `docs/01-architecture/03-core-data-model.md` §7,
//! `docs/01-architecture/05-prompt-conversion.md`.
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the `NOTICE` file in the crate root.

pub mod data_url;
mod error;
mod file_source;
mod message;
mod part;
pub mod prune;

pub use data_url::DataUrl;
pub use error::FileSourceError;
pub use error::InvalidDataContentError;
pub use ferrin_spec::ApprovalId;
pub use ferrin_spec::FileData;
pub use ferrin_spec::MediaType;
pub use ferrin_spec::ProviderOptions;
pub use ferrin_spec::ProviderReference;
pub use ferrin_spec::ToolCallId;
pub use ferrin_spec::ToolName;
pub use ferrin_spec::language_model::prompt::CustomPart;
pub use ferrin_spec::language_model::prompt::ReasoningPart;
pub use ferrin_spec::language_model::prompt::TextPart;
pub use ferrin_spec::language_model::prompt::ToolCallPart;
pub use ferrin_spec::language_model::prompt::ToolResultContentPart;
pub use ferrin_spec::language_model::prompt::ToolResultOutput;
pub use ferrin_spec::language_model::prompt::ToolResultPart;
pub use file_source::FileSource;
pub use message::AssistantContent;
pub use message::AssistantMessage;
pub use message::Message;
pub use message::MessagesExt;
pub use message::Role;
pub use message::SystemMessage;
pub use message::ToolMessage;
pub use message::UserContent;
pub use message::UserMessage;
pub use part::AssistantPart;
pub use part::FilePart;
pub use part::ImagePart;
pub use part::ReasoningFilePart;
pub use part::ToolApprovalRequest;
pub use part::ToolApprovalResponse;
pub use part::ToolPart;
pub use part::UserPart;
pub use prune::PruneOptions;
