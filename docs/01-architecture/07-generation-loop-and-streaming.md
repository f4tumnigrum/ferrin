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

1. Calls `prepare_step` with the `model`, completed steps, step number, current and initial `messages`, complete response history, tool context and runtime context. Message, instruction and context overrides persist into subsequent steps; model, tool selection/order and sampling overrides apply only to the current step (ADR 0021).
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
10. `stop_when` defaults to `step_count(1)`; evaluate configured predicates concurrently and await all before selecting whether any is true. `step_count(n)` matches exactly `n` completed steps, so `step_count(0)` does not stop a call after its first completed step. Other built-ins cover a named tool call and natural loop completion.

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

[Decision] Reject negative or non-finite `backoff_factor` values before invoking a provider. Compute a retry delay only after the error is classified as retryable and the retry budget permits another attempt. Saturate duration arithmetic and keep waits beyond the runtime clock's range cancellable; public delay calculation must not panic on extreme values. This preserves the reference SDK's terminal-error ordering while making Rust duration conversion explicit. Sources: Vercel AI SDK `6c6c221`, `retry-with-exponential-backoff.ts`; core retry boundary regressions.

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
    pub fn usage(&self) -> &Usage;            // summed across every step
    pub fn response_messages(&self) -> Vec<Message>;   // all steps
    pub fn warnings(&self) -> Vec<&Warning>;  // all steps, in order
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
    completion: Completion<O>,           // polling also drives the event pipeline
}

impl<O> StreamTextResult<O> {
    pub fn split(self) -> (EventStream, Completion<O>);
    pub fn events(&mut self) -> &mut EventStream;
    pub fn full_stream(&mut self) -> EventStream;
    pub fn into_completion(self) -> Completion<O>;
    pub async fn final_result(self) -> Result<GenerateTextResult<O>, Error>;
    pub fn text_stream(self) -> impl Stream<Item = Result<String, Error>>;   // consumes the result, forwards text deltas
    pub fn partial_output_stream(self) -> impl Stream<Item = PartialOutput<O>>;
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error>;     // drain everything
}

pub struct Completion<O> { /* final receiver and an owned event driver */ }
```

[Decision] ADR 0026 adds owned tee views: `full_stream`, `text_view`, `partial_output_view` and `element_view` retain an independent cursor without consuming the result. `split` returns an event cursor and a completion that drives another cursor when awaited; `into_completion` and `final_result` drive progress without an event consumer. No task is detached: the final cursor/driver drop releases the processor and its owned task set. Dropping one cursor leaves other owners usable. Lagging cursors buffer their unread events, matching the reference SDK tee semantics; applications must drop views they no longer need. Final results retain owned `O` and errors without a `Clone` requirement, so completion resolves once; callers retain and borrow the returned result for repeated reads.


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

[Decision] `UnicodeWords` uses locale-neutral `unicode-segmentation` word boundaries; custom `Detector` callbacks provide locale-tailored segmentation when required. No ICU dictionary data is bundled.

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

[Fact] The earlier smoothing implementation emitted metadata on the first resegmented chunk and flushed before later metadata-bearing deltas. ADR 0026 replaces that behavior with the reference SDK’s latest-metadata-at-flush semantics, recorded below (2026-09-17; `tests/suite/stream_metadata.rs`).

## 7. Implementation record (2026-09-17)

[Decision] [ADR 0021](../04-decisions/2026-09-17-0021-agent-runtime-context.md) defines persistent per-invocation state in `generate_text/inputs.rs::StepState`, shared by generation and streaming. `PrepareStepContext` exposes the evolving messages, instructions, `tools_context` and `runtime_context`; `initial_messages` and `response_messages` remain complete originals. A message replacement receives only new response messages on later steps. Model, tool choice/order/active set and sampling overrides retain their single-step behavior.

[Decision] `runtime_context(JsonValue)` is application lifecycle state separate from validated tool context; it reaches approval policies and lifecycle hooks but is never passed to model options or tool executors. `StepOverrides::with_runtime_context` replaces it; no override retains the previous value, while JSON `null` is an explicit value. `StepResult` and `StreamEvent::StartStep` capture both contexts with optional serde defaults. Application hooks see these snapshots; telemetry copies include runtime context only with `TelemetryOptions::include_runtime_context` and tool context only with `include_tools_context` (both false by default). Approval replay uses initial invocation contexts before `prepare_step`.

[Fact] Deterministic coverage is in `crates/ferrin-core/tests/suite/runtime_context.rs`: three-step compression and continued instructions/context in both loops, separate execution/approval contexts, per-call agent isolation, explicit null replacement, hook visibility, telemetry filtering and historical result deserialization. This does not verify live provider behavior.

## Implementation record (2026-09-17): result consumption parity

[Decision] `GenerateTextResult::usage` returns total usage; `warnings` collects references from all steps. Content, files, sources and static/dynamic tool call/result iterators span all steps, while text, reasoning, request/response, provider metadata and finish reason retain final-step semantics. The `final_step` alias uses the reference SDK name. Source: `generate-text-result.ts` and `stream-text.ts` in Vercel AI SDK `6c6c221`; core result aggregation regressions.

[Decision] `into_shared_completion` is available when `O: Send + Sync + 'static`: cloneable waiters resolve to the same `Arc<Result<GenerateTextResult<O>, Error>>`, without cloning output or errors. Ordinary completion continues to return an owned result once. Full/text/partial/element views have independent cursors; the final result remains the authoritative terminal error channel for filtered views. Sources: `src/stream_text/result.rs`, `result/tee.rs`, `tests/suite/stream_views.rs`.

[Decision] Retry attempt boundaries are intercepted outside user transforms. Each attempt creates fresh transform state; the boundary reaches the processor even when a user transform filters or reconstructs every event. `TransformContext::fail` stops the pipeline with the supplied error. Smoothing rejects empty regex matches and invalid detector byte lengths instead of silently accumulating text; cancellation interrupts smoothing delays. Source: Vercel AI SDK `6c6c221` retry segment and smoothing code, stream-transform parity regressions.

[Decision] `on_chunk` observes transformed public events, except internal retry boundaries; provider stream errors are observed before `on_error` at the retry layer even when recovery suppresses the error event. Unchanged terminal errors are not observed twice; an error replaced by a transform is a separate event and invokes both callbacks. Callback panics are isolated, preserving the provider failure and automatic retry budget. Source: Vercel AI SDK `6c6c221`, `errorsHandledForStreamRetry` and stream callback regressions.

[Decision] Smoothing retains the latest part metadata until its buffer flushes, including an empty metadata-only delta when a word/line already emptied the text buffer; ordinary split chunks do not carry that metadata. `UnicodeWords` emits the first UAX #29 segment immediately, including whitespace/punctuation segments, mirroring the reference segmenter strategy's first-segment behavior. It does not supply locale dictionaries or ICU tailoring; use a custom `Detector` for application-specific segmentation. Oversized delays remain cancellation-aware without clock overflow. Sources: Vercel AI SDK `smooth-stream.ts`, metadata and Unicode/timer regressions.
