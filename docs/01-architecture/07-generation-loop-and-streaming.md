# Generation loop and streaming pipeline

**English** | [Chinese](../zh-CN/01-architecture/07-generation-loop-and-streaming.md)

This document defines `ferrin-core::generate_text` and `ferrin-core::stream_text`. They share step semantics, tool execution, and stop conditions, differing only in model invocation and result delivery.

## 1. Step model

[Decision] One `generate_text` invocation contains multiple steps, each consisting of a model call followed by client tool execution. `StepResult` contains ordered `content`, text, reasoning, files, sources, static/dynamic tool calls, tool results/errors, normalized/raw finish reasons, usage, warnings, request, response (ID, timestamp, model ID, headers, body, messages), provider metadata, performance metrics, step number, and runtime context. Steps are the natural unit for progress, audit, and billing.

```rust
pub struct StepResult {
    pub step_number: u32,
    pub content: Vec<StepContent>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub warnings: Vec<Warning>,
    pub request: RequestMetadata,
    pub response: StepResponse,          // id, timestamp, model_id, headers, body(Option), messages: Vec<Message>
    pub provider_metadata: Option<ProviderMetadata>,
    pub performance: StepPerformance,
}

impl StepResult {
    pub fn text(&self) -> String;                       // concatenated text parts
    pub fn reasoning_text(&self) -> Option<String>;
    pub fn tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall>;
    pub fn tool_results(&self) -> impl Iterator<Item = &ToolResult>;
    pub fn tool_errors(&self) -> impl Iterator<Item = &ToolError>;
    pub fn files(&self) -> impl Iterator<Item = &GeneratedFile>;
    pub fn sources(&self) -> impl Iterator<Item = &Source>;
    pub fn response_messages(&self) -> Vec<Message>;
}
```

`StepContent` extends specification `Content` with `ToolCall(ParsedToolCall)` (parsed, validated input), `ToolResult`, `ToolError`, `ToolApprovalRequest`, and `ToolOutputDenied`.

## 2. Non-streaming loop

[Decision] Each step of `generate_text`:

1. Calls `prepare_step` with the `model`, completed steps, step number, `messages`, and runtime context. It may override `model`, `tool_choice`, `active_tools`, `system`, `messages`, `runtime_context`, and sampling parameters; other settings inherit outer values.
2. Converts messages into specification prompts, downloading URLs, and prepares tools and tool choice.
3. Calls `do_generate` under the retry policy and step timeout.
4. Parses tool calls, including repair, invalid flags, and tool-choice violations. `on_language_model_call_end` runs after parsing and before execution.
5. Resolves approval for each client call. Runs `not-applicable`/`approved` calls concurrently only for finish reasons `stop` and `tool-calls`; emits requests for `user-approval` and denial results for `denied`.
6. Tracks deferred provider tool results: add calls supporting deferred results that have no result this step; remove them when later results arrive.
7. Assembles `StepResult`, invokes `on_step_end`, and appends response messages.
8. Continues under these conditions:

```typescript
} while (
  clientToolOutputs.length + deniedToolApprovalResponses.length === clientToolCalls.length &&
  (clientToolCalls.length > 0 || pendingDeferredToolCalls.size > 0) &&
  !(await isStopConditionMet({ stopConditions, steps }))
);
```

All client calls have output or denial (no pending approval or missing `execute`), there are client calls or outstanding deferred results, and no stop condition is met.

9. On completion, sums `total_usage`. If `output` is configured, calls `Output::parse_complete` when the last finish reason is `stop`, or is not `tool-calls` and text is nonempty; otherwise skips parsing. Invokes `on_end`.
10. `stop_when` defaults to `step_count(1)`; any satisfied condition stops the loop. Built-ins cover step limits, a named tool call, and natural loop completion.

[Decision] `generate_text::run` implements these rules; `should_continue(&LoopState) -> bool` has tests for pending approval, missing executors, outstanding deferred results, and satisfied stop conditions.

```rust
pub trait StopCondition: Send + Sync {
    fn is_met(&self, steps: &[StepResult]) -> BoxFuture<'_, bool>;
}

pub fn step_count(n: u32) -> impl StopCondition;
pub fn has_tool_call(name: impl Into<ToolName>) -> impl StopCondition;
pub fn loop_finished() -> impl StopCondition;   // finish reason != tool-calls or no pending client tools
```

### 2.1 Retries

[Decision] Default to 2 retries, 2000 ms initial delay, multiplier 2. `ApiCallError::is_retryable` determines eligibility. Honor `retry-after-ms` or `retry-after` (seconds or HTTP date) when the resulting delay is 0–60 s; otherwise use backoff. Exhaustion returns `RetryError` with the reason (exhausted, non-retryable, or cancelled) and all errors. Cancellation aborts immediately. Two retries handle transient failures without minute-scale delays; respecting retry headers follows provider rate-limit guidance, while the 60 s cap rejects pathological waits.

```rust
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,                  // default 2
    pub initial_delay: Duration,           // default 2s
    pub backoff_factor: f64,               // default 2.0
    pub max_retry_after: Duration,         // default 60s
    pub jitter: Jitter,                    // default Jitter::None
}
```

[Decision] Disable jitter by default for deterministic tests; offer `Jitter::Full`. Retry individual model calls only, never tool execution.

### 2.2 Timeouts

[Decision] Configure total, step, first-chunk, inter-chunk, tool, and per-tool timeouts. Chunk timeouts apply only to streaming; total time includes tools. Timed cancellation uses tokens derived from the caller's token. Separate scopes distinguish an unresponsive provider, interrupted stream, and stuck tool.

```rust
#[derive(Debug, Clone, Default)]
pub struct Timeout {
    pub total: Option<Duration>,
    pub step: Option<Duration>,
    pub first_chunk: Option<Duration>,     // streaming only
    pub chunk: Option<Duration>,           // streaming only
    pub tool: Option<Duration>,
    pub per_tool: HashMap<ToolName, Duration>,
}

impl From<Duration> for Timeout { /* total only */ }
```

Timeouts produce `Error::Timeout { scope: Total | Step | FirstChunk | Chunk | Tool(name) }`; see [Concurrency, cancellation, and timeouts](16-concurrency-and-cancellation.md) for token relationships.

### 2.3 Results

[Decision] `GenerateTextResult` contains content, text, reasoning, files, sources, tool calls/results/errors, finish reason, last-step and total usage, warnings, request, response with messages, provider metadata, all steps, and structured output whose type is determined by generic `O`.

```rust
pub struct GenerateTextResult<O = ()> {
    pub steps: Vec<StepResult>,
    pub total_usage: Usage,
    pub output: O,                      // () when no Output configured
}

impl<O> GenerateTextResult<O> {
    pub fn last_step(&self) -> &StepResult;
    pub fn text(&self) -> String;
    pub fn finish_reason(&self) -> &FinishReason;
    pub fn usage(&self) -> &Usage;            // last step
    pub fn response_messages(&self) -> Vec<Message>;   // all steps
    pub fn warnings(&self) -> &[Warning];     // last step
}
```

[Decision] Express `output` generically: `()` without `Output`, `T` when configured, moving misuse of an unconfigured `output` from runtime to compile time.

### 2.4 `include` options

[Decision] `Include { request_body, request_messages, response_body }` controls retention of these fields, all `false` by default. Streaming replaces `response_body` with `raw_chunks`. These potentially large and sensitive fields are opt-in for debugging or audit.

## 3. Streaming pipeline

### 3.1 Stages

[Decision] `stream_text` passes the model stream through tool execution, multi-step stitching, resilience (errors and stream retries), stop gate, user transforms, output transform, and the event processor. Independent transforms make each concern testable and composable.

[Decision] Ferrin uses corresponding stages but delivers one event stream and a completion handle:

```rust
pub struct StreamTextResult<O = ()> {
    events: EventStream,                 // impl Stream<Item = StreamEvent>
    completion: Completion<O>,           // resolves after the stream is fully drained
}

impl<O> StreamTextResult<O> {
    pub fn split(self) -> (EventStream, Completion<O>);
    pub fn events(&mut self) -> &mut EventStream;
    pub fn text_stream(self) -> impl Stream<Item = Result<String, Error>>;   // consumes the result, forwards text deltas
    pub fn partial_output_stream(self) -> impl Stream<Item = PartialOutput<O>>;
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error>;     // drain everything
}

pub struct Completion<O>(oneshot::Receiver<Result<GenerateTextResult<O>, Error>>);
```

Rust streams are pull-based and single-consumer. Automatically driving multiple tee views requires unbounded buffering and implicit ownership. `split` allows event forwarding and result waiting in separate tasks; applications needing fan-out may use `tokio::sync::broadcast`.

### 3.2 Event types

[Decision] `StreamEvent` variants: `start`, `start-step {request, warnings}`, `text-start/text-delta/text-end`, `reasoning-start/delta/end`, `reasoning-file`, `file`, `source`, `custom`, `tool-input-start/delta/end`, `tool-call`, `tool-result` (including `preliminary`), `tool-error`, `tool-approval-request`, `tool-output-denied`, `finish-step {finish_reason, raw_finish_reason, usage, response, provider_metadata}`, `finish {finish_reason, total_usage}`, `error`, `abort`, and `raw`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum StreamEvent {
    Start { call_id: String },
    StartStep { step_number: u32, request: RequestMetadata, warnings: Vec<Warning> },
    TextStart { id: PartId, provider_metadata: Option<ProviderMetadata> },
    TextDelta { id: PartId, text: String, provider_metadata: Option<ProviderMetadata> },
    TextEnd { id: PartId, provider_metadata: Option<ProviderMetadata> },
    ReasoningStart { id: PartId, provider_metadata: Option<ProviderMetadata> },
    ReasoningDelta { id: PartId, text: String, provider_metadata: Option<ProviderMetadata> },
    ReasoningEnd { id: PartId, provider_metadata: Option<ProviderMetadata> },
    ReasoningFile(GeneratedFile),
    File(GeneratedFile),
    Source(Source),
    Custom { kind: CustomKind, provider_metadata: Option<ProviderMetadata> },
    ToolInputStart { id: ToolCallId, tool_name: ToolName, provider_executed: bool, dynamic: bool },
    ToolInputDelta { id: ToolCallId, delta: String },
    ToolInputEnd { id: ToolCallId },
    ToolCall(ParsedToolCall),
    ToolResult(ToolResult),                 // includes `preliminary`
    ToolError(ToolError),
    ToolApprovalRequest(ToolApprovalRequest),
    ToolOutputDenied { tool_call_id: ToolCallId, tool_name: ToolName, reason: Option<String> },
    FinishStep { step_number: u32, finish_reason: FinishReason, usage: Usage, response: StepResponseMetadata, provider_metadata: Option<ProviderMetadata> },
    Finish { finish_reason: FinishReason, total_usage: Usage },
    Error { error: StreamErrorInfo },
    Abort,
    Raw { raw_value: JsonValue },
}
```

All events are serializable for SSE or WebSocket forwarding. `StreamErrorInfo` projects `Error` into serializable kind, message, retryability, and status code.

### 3.3 Tool execution stage

[Decision] Forward chunks immediately. Queue valid, locally executable, non-provider calls with `not-applicable` approval; emit approval-request events when required. At `model-call-end`, if its finish reason allows execution, start tools concurrently and inject `tool-result`/`tool-error`, followed by `tool-execution-end {tool_call_id, tool_execution_ms}`. Clear pending execution on `AttemptBoundary` because that attempt is discarded. Immediate forwarding avoids blocking text, and waiting for model-call completion ensures the finish reason is known.

[Decision] Start tool tasks in `JoinSet` and inject results through bounded `mpsc`. Tasks own `Arc<Tool>` and cloned `ToolContext`; cancellation aborts all tasks.

### 3.4 Resilience and stream retries

[Decision] `on_error` may request retry. `stream_retries`, disabled by default, controls automatic retries of the current step after a provider error during streaming. Earlier steps remain. Already emitted output cannot be withdrawn, but is excluded from recovered step results, structured parsing, response messages, and later steps to avoid double counting.

[Decision] Provide `stream_retries(u32)` and `on_error(fn) -> ErrorDecision::{Continue, Retry}`. Internal `AttemptBoundary` tells the event processor to discard the failed attempt's accumulated content. Applications must opt in because consumed partial output is replayed.

### 3.5 Part ID remapping

[Fact] Provider text/reasoning IDs are unique only within one call; Anthropic uses content-block indexes. [Decision] Remap collisions across steps in the core.

The stitching stage tracks used IDs and appends `-<n>` on collision.

### 3.6 User transforms

[Decision] `transform` accepts one or more transforms applied in order, preserving event structure. Built-in `smooth_stream` buffers text/reasoning and emits words or lines; default delay is 10 ms, with word, line, regex, Unicode, or custom segmentation. Flush `provider_metadata` with the buffer when type or ID changes. Resegmentation smooths unstable provider chunk boundaries for display.

```rust
pub trait StreamTransform: Send + Sync {
    fn apply(&self, input: EventStream, ctx: TransformContext) -> EventStream;
}

pub fn smooth_stream(config: SmoothStreamConfig) -> impl StreamTransform;

pub enum Chunking {
    Word,
    Line,
    Regex(regex::Regex),
    UnicodeWords,                 // unicode-segmentation word boundaries, for CJK
    Detector(Arc<dyn Fn(&str) -> Option<usize> + Send + Sync>),
}
```

[Decision] Use `unicode-segmentation` word boundaries as the equivalent of `Intl.Segmenter`, without ICU data.

### 3.7 Event processor

The event processor alone owns mutable aggregation state: it accumulates step content, creates `StepResult`, tracks IDs, computes first-chunk latency, inter-chunk statistics and output token rate, totals usage, sends final results to `Completion` after `Finish`, invokes `on_step_end`/`on_end`/`on_abort`, and dispatches telemetry.

### 3.8 Startup semantics

[Decision] `stream_text(...).await` returns `Result<StreamTextResult, Error>` after the first model request is established (acceptable response headers, or failure after retries). Later errors arrive through `StreamEvent::Error` and `Completion`. Callers can use `?` for configuration/authentication errors, matching the pattern of awaiting `reqwest::RequestBuilder::send()` before consuming its body. Synchronous handle creation with background startup and stream-only startup errors was rejected; see [ADR 0005](../04-decisions/2026-09-13-0005-stream-result-delivery.md).

## 4. Callbacks

[Decision] Hooks: `on_start`, `on_step_start`, `on_language_model_call_start`, `on_language_model_call_end`, `on_tool_execution_start`, `on_tool_execution_end`, `on_step_end`, and `on_end`; streaming adds `on_chunk`, `on_error`, and `on_abort`.

[Decision] `Hooks` holds `Arc<dyn Fn(...) -> BoxFuture<'_, ()>>`; builder methods accept forms such as `on_step_end(|step| async move {...})`. The core awaits callbacks, pausing stream processing until they finish.

## 5. Verification items

- [Fact] (PV-006, `verification/pv006-joinset`, release build, macOS arm64) With 200 parallel tools (1–20 ms delay, 4 KiB results) injecting through `JoinSet` and bounded `mpsc` into a slow consumer, capacities 1/64/1024 showed no measurable duration (284–308 ms) or peak RSS (2.9 MiB) difference. At 1000 tasks, results were also equivalent (1.40–1.42 s, 6.7 MiB). Results arrive in completion order, with roughly 30% inversions relative to dispatch. Results wait inside tasks, so concurrency, not channel capacity, determines memory.
- [Decision] Keep default channel capacity 64. Bound memory with `ToolExecutionOptions::max_concurrency`, unlimited by default.
- [Decision] (PV-007) Do not add `start_eager()`. Startup completes after the first `model.stream()` returns; with `simulate_streaming`, this follows the entire `generate()` call. The middleware documents this inherent delay. Applications needing an immediate handle can start the call in their own task; the library keeps one startup semantic.

## 6. Implementation record (2026-09-13)

- [Fact] `StepResult` adds `model: ModelIdentity { provider, model_id }` (the actual step model after any `prepare_step` override), `request: StepRequest { body, messages }`, and `response: StepResponse { id, timestamp, model_id, headers, body, messages }`. `ToolResult` adds `execution_ms: Option<u64>` for client tools only.
- [Fact] Beyond section 1, `StepContent` includes `ToolApprovalResponse` (replay decision), `Reasoning`, `ReasoningFile`, `File`, `Custom`, and `Source`.
- [Decision] `Include::default()` equals `Include::none()`, with all flags `false`. `request_messages` retains the sent `Vec<Message>` in `StepRequest.messages`.
- [Decision] Approval replay tool messages (`replay_tool_message`, with approved execution results and `execution-denied` outputs) precede all step assistant/tool messages in the invocation's response messages. Replay happens before the first model call, so appending `response_messages()` preserves causal order.
- [Decision] `retry()` wraps operations returning core `Error`. Only `Error::Provider` participates in retry classification; `Error::Cancelled` aborts immediately and other core errors pass through. `retry_with()` extends classification, for example treating empty image results as retryable.
- [Decision] If required `tool_choice` is unmet, end the step with `Error::ToolChoiceNotSatisfied { expected: Option<ToolName> }`, checked by the streaming event processor as well. This distinguishes model refusal to call a tool from normal completion.
- [Fact] Additional `StreamEvent` variants are `RetryAttempt { step_number, attempt, request, warnings }` (failed content stays in the stream but not step results) and `ToolApprovalResponse`. `ToolInputStart` carries `provider_executed`, `dynamic`, and `provider_metadata`.
- [Decision] Buffer `ToolInputStart`/`Delta`/`End` by ID; at `ToolInputEnd`, parse and validate into `ToolCall(ParsedToolCall)`. Direct provider `tool-call` chunks use the same `parse_tool_call` path, sharing repair and refinement with non-streaming calls.
- [Decision] `TransformContext::stop()` cancels the invocation, model stream, and pending tools, ending with `Error::Cancelled`. An internal `stop` token gates subsequent events; transforms should end their own output streams. Merely ending output would leak tasks and allow billable provider generation to continue.
- [Decision] Startup readiness is the first `do_stream()` returning `Ok` or a retried `Err`, as in section 3.8. Errors after that point but before the first `StreamStart` arrive through `StreamEvent::Error` and `Completion`.
- [Fact] `ferrin_core::clock::Clock` (`fn now(&self) -> DateTime<Utc>`, blanket-implemented for `Fn() -> DateTime<Utc>`) injects step timestamps and performance clocks. Fixed test clocks remove timestamp differences from snapshots.

[Decision] Both generation loops run the same required/named tool-choice completion check before executing queued tools, including when a provider returns text only or refuses the request.
