//! Ferrin tool system.
//!
//! Tool definitions ([`Tool`], [`ToolKind`], [`ToolSet`]), the execution
//! contract ([`ToolExecute`], [`ToolOutput`], [`ToolContext`], [`ToolError`]),
//! approval declarations ([`NeedsApproval`]), model-output normalisation,
//! caller restrictions, tool fingerprints and (feature `sandbox`) the
//! [`Sandbox`] trait with a local-process implementation.
//!
//! Tool call parsing, approval resolution, execution scheduling and repair
//! live in the core crate; this crate only describes tools and runs one
//! execution when asked.
//!
//! Design: `docs/01-architecture/06-tool-system.md`, ADR 0012.
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the `NOTICE` file in the crate root.

mod builder;
pub mod callers;
mod error;
mod execute;
pub mod fingerprint;
pub mod model_output;
#[cfg(feature = "sandbox")]
pub mod sandbox;
mod set;
mod tool;

pub use builder::ToolBuilder;
pub use callers::PreparedToolCallers;
pub use callers::ToolCaller;
pub use callers::ToolCallerDefinition;
pub use callers::ToolCallers;
pub use error::DuplicateToolError;
pub use error::ToolError;
pub use execute::ToolContext;
pub use execute::ToolExecute;
pub use execute::ToolOutput;
pub use execute::ToolOutputStream;
pub use execute::execute_to_completion;
pub use ferrin_schema::JsonSchema;
pub use ferrin_schema::Schema;
pub use fingerprint::ToolDrift;
pub use model_output::ErrorMode;
#[cfg(feature = "sandbox")]
pub use sandbox::LocalProcessSandbox;
#[cfg(feature = "sandbox")]
pub use sandbox::Sandbox;
#[cfg(feature = "sandbox")]
pub use sandbox::SandboxProcess;
pub use set::ToolSet;
pub use tool::ApprovalFn;
pub use tool::Description;
pub use tool::DescriptionContext;
pub use tool::DescriptionFn;
pub use tool::InputAvailableHook;
pub use tool::InputDeltaHook;
pub use tool::InputStartHook;
pub use tool::ModelOutputArgs;
pub use tool::NeedsApproval;
pub use tool::ToModelOutputFn;
pub use tool::Tool;
pub use tool::ToolHooks;
pub use tool::ToolKind;
