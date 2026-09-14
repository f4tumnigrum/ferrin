//! GenAI semantic-convention names used by this crate.
//!
//! The names follow the OpenTelemetry GenAI semantic conventions repository
//! (`docs/gen-ai/gen-ai-spans.md`, `docs/gen-ai/gen-ai-metrics.md`, status
//! "Development"). They are defined here because the `GEN_AI_*` constants of
//! `opentelemetry-semantic-conventions` are deprecated. Ferrin-specific
//! attributes use the `ferrin.*` prefix.

/// Instrumentation scope name of the tracer and meter.
pub const SCOPE_NAME: &str = "ferrin-otel";

/// `gen_ai.operation.name`: the operation being performed.
pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
/// `gen_ai.provider.name`: the GenAI provider.
pub const GEN_AI_PROVIDER_NAME: &str = "gen_ai.provider.name";
/// `gen_ai.request.model`: the requested model.
pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
/// `gen_ai.response.model`: the model that produced the response.
pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";
/// `gen_ai.response.id`: the response identifier.
pub const GEN_AI_RESPONSE_ID: &str = "gen_ai.response.id";
/// `gen_ai.response.finish_reasons`: finish reasons, one per generation.
pub const GEN_AI_RESPONSE_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";
/// `gen_ai.usage.input_tokens`: input tokens including cached tokens.
pub const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
/// `gen_ai.usage.output_tokens`: output tokens.
pub const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
/// `gen_ai.token.type`: `input` or `output` (metric attribute).
pub const GEN_AI_TOKEN_TYPE: &str = "gen_ai.token.type";
/// `gen_ai.tool.name`: the executed tool.
pub const GEN_AI_TOOL_NAME: &str = "gen_ai.tool.name";
/// `gen_ai.tool.call.id`: the tool call identifier.
pub const GEN_AI_TOOL_CALL_ID: &str = "gen_ai.tool.call.id";
/// `gen_ai.tool.type`: the tool type (`function` for Ferrin tools).
pub const GEN_AI_TOOL_TYPE: &str = "gen_ai.tool.type";
/// `gen_ai.tool.call.arguments`: tool input (opt-in).
pub const GEN_AI_TOOL_CALL_ARGUMENTS: &str = "gen_ai.tool.call.arguments";
/// `gen_ai.tool.call.result`: tool output (opt-in).
pub const GEN_AI_TOOL_CALL_RESULT: &str = "gen_ai.tool.call.result";
/// `error.type`: low-cardinality error class.
pub const ERROR_TYPE: &str = "error.type";

/// `ferrin.call_id`: the Ferrin call id.
pub const FERRIN_CALL_ID: &str = "ferrin.call_id";
/// `ferrin.step_number`: the zero-based step index.
pub const FERRIN_STEP_NUMBER: &str = "ferrin.step_number";
/// `ferrin.function_id`: the function id from the telemetry options.
pub const FERRIN_FUNCTION_ID: &str = "ferrin.function_id";
/// `ferrin.streaming`: whether the model call was streamed.
pub const FERRIN_STREAMING: &str = "ferrin.streaming";

/// Operation name of language model calls.
pub const OPERATION_CHAT: &str = "chat";
/// Operation name of embedding calls.
pub const OPERATION_EMBEDDINGS: &str = "embeddings";
/// Operation name of tool executions.
pub const OPERATION_EXECUTE_TOOL: &str = "execute_tool";
/// Operation name of rerank calls (not a well-known value; custom values are
/// permitted by the conventions).
pub const OPERATION_RERANK: &str = "rerank";

/// Tool type recorded for Ferrin tools.
pub const TOOL_TYPE_FUNCTION: &str = "function";

/// `gen_ai.client.token.usage` histogram (`{token}`).
pub const METRIC_TOKEN_USAGE: &str = "gen_ai.client.token.usage";
/// `gen_ai.client.operation.duration` histogram (`s`).
pub const METRIC_OPERATION_DURATION: &str = "gen_ai.client.operation.duration";
/// `gen_ai.client.operation.time_to_first_chunk` histogram (`s`).
pub const METRIC_TIME_TO_FIRST_CHUNK: &str = "gen_ai.client.operation.time_to_first_chunk";
/// `gen_ai.execute_tool.duration` histogram (`s`).
pub const METRIC_EXECUTE_TOOL_DURATION: &str = "gen_ai.execute_tool.duration";

/// Recommended bucket boundaries of `gen_ai.client.token.usage`.
pub const TOKEN_USAGE_BOUNDARIES: [f64; 14] = [
    1.0,
    4.0,
    16.0,
    64.0,
    256.0,
    1024.0,
    4096.0,
    16384.0,
    65536.0,
    262_144.0,
    1_048_576.0,
    4_194_304.0,
    16_777_216.0,
    67_108_864.0,
];

/// Recommended bucket boundaries of the duration histograms (seconds).
pub const DURATION_BOUNDARIES: [f64; 14] = [
    0.01, 0.02, 0.04, 0.08, 0.16, 0.32, 0.64, 1.28, 2.56, 5.12, 10.24, 20.48, 40.96, 81.92,
];
