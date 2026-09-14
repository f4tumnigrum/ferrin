//! Multi-step text generation (`generate_text`).
//!
//! The loop converts the prompt, calls the model, parses tool calls, resolves
//! approvals, executes client tools and repeats until no client tool calls
//! remain or a stop condition is met.
//!
//! Design: `docs/01-architecture/07-generation-loop-and-streaming.md`,
//! `docs/01-architecture/06-tool-system.md`.

pub(crate) mod approval;
mod builder;
pub(crate) mod config;
pub(crate) mod execute_tool;
pub(crate) mod inputs;
pub(crate) mod parse_tool_call;
mod prepare_step;
pub(crate) mod replay;
pub(crate) mod response_messages;
mod result;
pub(crate) mod run;
mod step;
mod stop_condition;
pub(crate) mod tools;

pub use approval::ApprovalContext;
pub use approval::ApprovalPolicy;
pub use approval::ApprovalPolicyFn;
pub use approval::ApprovalStatus;
pub use approval::approval_policy;
pub use approval::signature::SIGNATURE_DOMAIN;
pub use builder::GenerateText;
pub use builder::generate_text;
pub use config::Include;
pub use parse_tool_call::RefineToolInputFn;
pub use parse_tool_call::RefineToolInputs;
pub use parse_tool_call::RepairRequest;
pub use parse_tool_call::ToolCallRepair;
pub use prepare_step::PrepareStep;
pub use prepare_step::PrepareStepContext;
pub use prepare_step::StepOverrides;
pub use result::GenerateTextResult;
pub use step::ChunkTimingStats;
pub use step::GeneratedFile;
pub use step::ParsedToolCall;
pub use step::StepContent;
pub use step::StepPerformance;
pub use step::StepRequest;
pub use step::StepResponse;
pub use step::StepResult;
pub use step::ToolApprovalRequestContent;
pub use step::ToolApprovalResponseContent;
pub use step::ToolErrorInfo;
pub use step::ToolExecutionError;
pub use step::ToolOutputDenied;
pub use step::ToolResult;
pub use stop_condition::HasToolCall;
pub use stop_condition::Never;
pub use stop_condition::StepCount;
pub use stop_condition::StopCondition;
pub use stop_condition::has_any_tool_call;
pub use stop_condition::has_tool_call;
pub(crate) use stop_condition::is_stop_condition_met;
pub use stop_condition::never;
pub use stop_condition::step_count;
