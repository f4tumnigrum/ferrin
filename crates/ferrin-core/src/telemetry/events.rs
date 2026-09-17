//! Telemetry event payloads.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use ferrin_message::Message;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::CallOptionsRecord;
use serde::Deserialize;
use serde::Serialize;

use crate::error::Error;
use crate::generate_text::StepContent;
use crate::generate_text::StepPerformance;
use crate::generate_text::StepResult;
use crate::generate_text::ToolErrorInfo;

/// Provider and model ids of the model handling a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    /// Provider id.
    pub provider: ProviderId,
    /// Model id.
    pub model_id: ModelId,
}

impl ModelIdentity {
    /// Creates an identity.
    #[must_use]
    pub fn new(provider: impl Into<ProviderId>, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }
}

/// Inputs recorded when `record_inputs` is enabled.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedInputs {
    /// System instructions.
    pub system: Option<crate::prompt::Instructions>,
    /// The initial messages.
    pub messages: Arc<[Message]>,
}

/// A call started.
#[derive(Debug, Clone)]
pub struct StartEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Function id from the telemetry options.
    pub function_id: Option<String>,
    /// The model.
    pub model: ModelIdentity,
    /// Inputs (only when `record_inputs`).
    pub inputs: Option<RecordedInputs>,
    /// Metadata from the telemetry options.
    pub metadata: BTreeMap<String, JsonValue>,
}

/// A step started.
#[derive(Debug, Clone)]
pub struct StepStartEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Zero-based step index.
    pub step_number: u32,
    /// The model of this step.
    pub model: ModelIdentity,
    /// Messages sent to the model (only when `record_inputs`).
    pub messages: Option<Arc<[Message]>>,
}

/// A model call is about to be made.
#[derive(Debug, Clone)]
pub struct ModelCallStartEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Zero-based step index.
    pub step_number: u32,
    /// The model.
    pub model: ModelIdentity,
    /// Serializable snapshot of the call options (only when `record_inputs`).
    pub call_options: Option<CallOptionsRecord>,
}

/// A model call finished and its tool calls were parsed.
#[derive(Debug, Clone)]
pub struct ModelCallEndEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Zero-based step index.
    pub step_number: u32,
    /// The model.
    pub model: ModelIdentity,
    /// Parsed content (only when `record_outputs`).
    pub content: Option<Vec<StepContent>>,
    /// Finish reason.
    pub finish_reason: FinishReason,
    /// Usage.
    pub usage: Usage,
    /// Response metadata.
    pub response: ResponseMetadata,
    /// Timing.
    pub performance: StepPerformance,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// A tool execution is about to start.
#[derive(Debug, Clone)]
pub struct ToolExecutionStartEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Tool call id.
    pub tool_call_id: ToolCallId,
    /// Tool name.
    pub tool_name: ToolName,
    /// Input (only when `record_inputs`).
    pub input: Option<JsonValue>,
}

/// A tool execution finished.
#[derive(Debug, Clone)]
pub struct ToolExecutionEndEvent {
    /// Application runtime state (available to hooks; telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// Tool call id.
    pub tool_call_id: ToolCallId,
    /// Tool name.
    pub tool_name: ToolName,
    /// Output on success (only when `record_outputs`).
    pub output: Option<ToolOutcome>,
    /// Error on failure.
    pub error: Option<ToolErrorInfo>,
    /// Execution time.
    pub duration: Duration,
}

/// The successful outcome of a tool execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    /// The final output value.
    pub output: JsonValue,
}

/// A step finished.
#[derive(Debug, Clone)]
pub struct StepEndEvent {
    /// Call id.
    pub call_id: String,
    /// The step.
    pub step: Arc<StepResult>,
}

/// A call finished.
#[derive(Debug, Clone)]
pub struct EndEvent {
    /// Application runtime state of the final step (telemetry requires `include_runtime_context`).
    pub runtime_context: Option<JsonValue>,
    /// Call id.
    pub call_id: String,
    /// All steps.
    pub steps: Arc<[StepResult]>,
    /// Usage summed over all steps.
    pub total_usage: Usage,
    /// Structured output as JSON (only when `record_outputs` and configured).
    pub output_recorded: Option<JsonValue>,
}

/// A streaming call was aborted by cancellation.
#[derive(Debug, Clone)]
pub struct AbortEvent {
    /// Call id.
    pub call_id: String,
    /// Steps completed before the abort.
    pub steps_completed: u32,
}

/// Where an error occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ErrorPhase {
    /// Prompt standardization or conversion.
    Prompt,
    /// The model call.
    ModelCall,
    /// Tool execution.
    ToolExecution,
    /// Structured output parsing.
    Output,
    /// The stream pipeline.
    Stream,
}

/// An error occurred.
#[derive(Debug)]
pub struct ErrorEvent<'a> {
    /// Call id.
    pub call_id: &'a str,
    /// The error.
    pub error: &'a Error,
    /// Where it occurred.
    pub phase: ErrorPhase,
}

/// An embedding call started.
#[derive(Debug, Clone)]
pub struct EmbedStartEvent {
    /// Call id.
    pub call_id: String,
    /// The model.
    pub model: ModelIdentity,
    /// Number of values.
    pub value_count: usize,
    /// The values (only when `record_inputs`).
    pub values: Option<Vec<String>>,
}

/// An embedding call finished.
#[derive(Debug, Clone)]
pub struct EmbedEndEvent {
    /// Call id.
    pub call_id: String,
    /// Number of embeddings.
    pub embedding_count: usize,
    /// Tokens used, if reported.
    pub tokens: Option<u64>,
    /// Wall time.
    pub duration: Duration,
}

/// A rerank call started.
#[derive(Debug, Clone)]
pub struct RerankStartEvent {
    /// Call id.
    pub call_id: String,
    /// The model.
    pub model: ModelIdentity,
    /// Number of documents.
    pub document_count: usize,
    /// The query (only when `record_inputs`).
    pub query: Option<String>,
}

/// A rerank call finished.
#[derive(Debug, Clone)]
pub struct RerankEndEvent {
    /// Call id.
    pub call_id: String,
    /// Number of ranked documents.
    pub ranked_count: usize,
    /// Wall time.
    pub duration: Duration,
}
