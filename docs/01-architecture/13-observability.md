# Observability

**English** | [Chinese](../zh-CN/01-architecture/13-observability.md)

Core telemetry lives in `ferrin-core::telemetry`; OpenTelemetry export lives in `ferrin-otel`.

## 1. Telemetry integration interface

[Decision] `Telemetry` supplies optional lifecycle callbacks (start, step start/end, model call start/end, tool start/end, embedding/reranking start/end, end, abort, error) and async context wrappers `execute_language_model_call`/`execute_tool`. `TelemetryOptions` controls enablement, input/output recording, `function_id`, `metadata`, runtime/tool context inclusion, and integrations. Callbacks cover lifecycle boundaries; wrappers establish parent/child span context.

```rust
pub trait Telemetry: Send + Sync + 'static {
    fn on_start<'a>(&'a self, event: &'a StartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_step_start<'a>(&'a self, event: &'a StepStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_language_model_call_start<'a>(&'a self, event: &'a ModelCallStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_language_model_call_end<'a>(&'a self, event: &'a ModelCallEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_tool_execution_start<'a>(&'a self, event: &'a ToolExecutionStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_tool_execution_end<'a>(&'a self, event: &'a ToolExecutionEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_step_end<'a>(&'a self, event: &'a StepEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_embed_start<'a>(&'a self, event: &'a EmbedStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_embed_end<'a>(&'a self, event: &'a EmbedEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_rerank_start<'a>(&'a self, event: &'a RerankStartEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_rerank_end<'a>(&'a self, event: &'a RerankEndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_end<'a>(&'a self, event: &'a EndEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_abort<'a>(&'a self, event: &'a AbortEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_error<'a>(&'a self, event: &'a ErrorEvent<'_>) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }

    /// Runs a model call inside integration-specific context (e.g. an OTel span).
    fn execute_language_model_call<'a>(
        &'a self,
        ctx: &'a ModelCallContext,
        call: BoxFuture<'a, Result<ModelCallOutcome, Error>>,
    ) -> BoxFuture<'a, Result<ModelCallOutcome, Error>> { call }

    fn execute_tool<'a>(
        &'a self,
        ctx: &'a ToolExecutionContext,
        call: BoxFuture<'a, Result<ToolOutcome, ToolError>>,
    ) -> BoxFuture<'a, Result<ToolOutcome, ToolError>> { call }
}

#[derive(Clone, Default)]
pub struct TelemetryOptions {
    pub enabled: bool,
    pub record_inputs: bool,
    pub record_outputs: bool,
    pub function_id: Option<String>,
    pub metadata: BTreeMap<String, JsonValue>,
    pub include_runtime_context: bool,
    pub include_tools_context: bool,
    pub integrations: Vec<Arc<dyn Telemetry>>,
}
```

[Decision] ([ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md)) Lifecycle callbacks return `BoxFuture<'a, ()>`, borrowing the integration and event. Dispatch invokes integrations in registration order and awaits all futures concurrently; unwinding invocation or polling panics are isolated after the panic handler runs. Execution wrappers retain their result-bearing futures, with the last integration outermost. This follows `packages/ai/src/telemetry/create-telemetry-dispatcher.ts` and `util/merge-callbacks.ts` at reference revision `6c6c221`; `panic = "abort"` cannot be isolated.

[Decision] No global telemetry registry. Inject integrations through call, agent, or registry-middleware `TelemetryOptions::integrations`. Applications can set process defaults in their own builder wrappers, keeping global mutable state out of the library.

## 2. Event payloads

Selected event fields:

| Event | Fields |
| --- | --- |
| `StartEvent` | `call_id`, `function_id`, `model: ModelIdentity {provider, model_id}`, `inputs: Option<RecordedInputs>` gated by `record_inputs`, `metadata` |
| `StepStartEvent` | `call_id`, `step_number`, `model`, `messages: Option<Arc<[Message]>>` |
| `ModelCallStartEvent` | `call_id`, `step_number`, serializable `call_options_snapshot` |
| `ModelCallEndEvent` | `content`, `finish_reason`, `usage`, `response`, `performance`, `warnings` |
| `ToolExecutionStartEvent` | `tool_call_id`, `tool_name`, `input: Option<JsonValue>` |
| `ToolExecutionEndEvent` | `tool_call_id`, `tool_name`, `output: Option<ToolOutcome>`, `duration` |
| `StepEndEvent` | `step: Arc<StepResult>` |
| `EndEvent` | `steps`, `total_usage`, `output_recorded: Option<JsonValue>` |
| `AbortEvent` | `call_id`, `steps_completed` |
| `ErrorEvent` | `call_id`, `error: &Error`, `phase: ErrorPhase` |

## 3. Built-in tracing

[Decision] The core always creates `tracing` spans/events independently of telemetry integrations, following OpenTelemetry GenAI `gen_ai.*` conventions. Applications gain basic observability with `tracing-subscriber` or `tracing-opentelemetry`. Tracing is the Rust ecosystem standard; coding rules require function-level `#[tracing::instrument]`.

| Span | Trigger | Key fields |
| --- | --- | --- |
| `ferrin.generate_text` / `ferrin.stream_text` | Invocation entry | `gen_ai.operation.name`, `ferrin.function_id`, `gen_ai.request.model`, `gen_ai.provider.name` |
| `ferrin.step` | Each step | `ferrin.step_number` |
| `ferrin.model_call` | `do_generate`/`do_stream` | Sampling `gen_ai.request.*`, `gen_ai.response.id`, `gen_ai.response.finish_reasons`, `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `ferrin.time_to_first_output_ms` |
| `ferrin.tool` | Tool execution | `gen_ai.tool.name`, `gen_ai.tool.call.id`, `ferrin.tool.duration_ms` |
| `ferrin.modality` (revised 2026-09-13 from `ferrin.embed`/`ferrin.rerank`/`ferrin.image`, etc.; section 8 and [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 5) | Other modalities | `gen_ai.operation.name`, `gen_ai.request.model`, `gen_ai.provider.name` |

Record content only when `record_inputs`/`record_outputs` is true, using target `ferrin::telemetry::content` so subscribers can filter it.

## 4. Warning logs

[Fact] Developers need adapter warnings about unsupported options, compatibility mappings, and deprecations, but libraries should not write directly to the console.

[Decision] Log one `tracing::warn!(target: "ferrin::warnings", ...)` event per warning, with `category`, `feature`, `provider`, and `model_id`; omit opaque warning descriptions. Applications control filters; no global switch.

## 5. Performance metrics

[Decision] Step metrics include response time, time to first output, output token rate and effective rate including initial latency, input token rate, effective total token rate, and output-chunk interval statistics (minimum, maximum, mean, p50/p90/p99, count). Timestamps and usage provide these comparisons without extra requests.

```rust
pub struct StepPerformance {
    pub response_time: Duration,
    pub time_to_first_output: Option<Duration>,
    pub output_tokens_per_second: Option<f64>,
    pub effective_output_tokens_per_second: f64,
    pub input_tokens_per_second: Option<f64>,
    pub effective_total_tokens_per_second: f64,
    pub time_between_output_chunks: Option<ChunkTimingStats>,
}
```

## 6. `ferrin-otel`

`ferrin_otel::OtelTelemetry` implements `Telemetry` (section 9):

- Execution wrappers create child spans. Use current `tracing` span context via `tracing-opentelemetry` 0.33.0 when its layer is installed, otherwise `opentelemetry::Context::current()`. Run calls with `FutureExt::with_context`.
- Follow GenAI attribute names; Ferrin-specific fields use `ferrin.*` (constants in `ferrin_otel::semconv`).
- Export histograms `gen_ai.client.token.usage`, `gen_ai.client.operation.duration`, `gen_ai.client.operation.time_to_first_chunk`, and `gen_ai.execute_tool.duration`.

[Fact] (PV-014, `verification/pv014-otel`) `opentelemetry` 0.32.0, `opentelemetry_sdk` 0.32.1, and `tracing-opentelemetry` 0.33.0 compile together and record spans. All `GEN_AI_*` constants in `opentelemetry-semantic-conventions` 0.32.1 are deprecated, with comments pointing to the separate GenAI conventions repository.

[Decision] Define GenAI constants locally in `semconv.rs` (`gen_ai.operation.name`, `gen_ai.provider.name`, `gen_ai.request.model`, `gen_ai.usage.input_tokens`, etc.), removing the dependency on deprecated `opentelemetry-semantic-conventions` constants.

## 7. Example

```rust
let result = ferrin::generate_text(&model)
    .prompt("Explain backpressure in two sentences.")
    .telemetry(TelemetryOptions {
        enabled: true,
        record_inputs: false,
        record_outputs: true,
        function_id: Some("docs.explain".into()),
        integrations: vec![Arc::new(ferrin_otel::OtelTelemetry::default())],
        ..Default::default()
    })
    .await?;
```

## 8. Implementation record (2026-09-13, ferrin-core)

- [Decision] ([ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 5) Non-text calls share `ferrin.modality`, with operation names `embed`, `image`, `speech`, `transcription`, `rerank`, `video`, `upload_file`, `get_file_metadata`, `download_file`, `delete_file`, `upload_skill`, `start_batch`, `get_batch_status`, `get_batch_results`, `cancel_batch`, and `list_batches`.
- [Decision] Create one `ferrin.model_call` span and corresponding telemetry events per `do_generate`/`do_stream` attempt. One span spanning retries would hide individual duration and errors.
- [Fact] `TelemetryDispatcher` emits modality `on_embed_start/on_embed_end`, `on_rerank_start/on_rerank_end` (call ID, model identity, counts, duration; inputs only when enabled), and `on_error(ErrorEvent { call_id, error, phase: ErrorPhase::{Prompt, ModelCall, ToolExecution, Output, Stream} })`.
- [Fact] `spans::log_warnings(&[Warning], &ModelIdentity)` centralizes warning logs, called after each non-text model call.

## 9. Implementation record (2026-09-14, ferrin-otel)

- [Fact] GenAI conventions moved from `opentelemetry.io` to `open-telemetry/semantic-conventions-genai`, files `docs/gen-ai/gen-ai-spans.md` and `docs/gen-ai/gen-ai-metrics.md` (read 2026-09-14; Development status). Inference spans SHOULD be named `{gen_ai.operation.name} {gen_ai.request.model}` with kind `CLIENT`; require operation/provider, conditionally model and `error.type`, and recommend response ID/model, finish reasons (`string[]`), and `input`/`output` tokens. Input/`output` messages are opt-in. Tool spans use `execute_tool {gen_ai.tool.name}`, kind `INTERNAL`, required operation `execute_tool` and tool name, recommended call ID/type (`function`/`extension`/`datastore`)/description, and opt-in arguments/results (JSON strings permitted). Token histograms use `{token}`, `gen_ai.token.type` `input`/`output`, and recommended buckets 1, 4, 16, …, 67108864. Operation duration and first-chunk latency use seconds (first-chunk streaming only); tool duration adds tool name/type and error type. Duration buckets are 0.01, 0.02, 0.04, …, 81.92. Known operations include `chat`, `generate_content`, `embeddings`, `execute_tool`, and `invoke_agent`; custom values are allowed.
- [Decision] Record all language calls as operation `chat`, span `chat {model_id}`, because Ferrin does not distinguish chat from generate-content interfaces. Embeddings use `embeddings`, reranking uses custom `rerank`. Error types/status descriptions use `ErrorKind::as_str()` (`provider`, `timeout`, `cancelled`, etc.) or tool variant names (`message`, `json`, `timeout`, `cancelled`), never error messages.
- [Decision] Streaming spans remain open after the execution wrapper returns, marked `ferrin.streaming = true` and keyed by `(call_id, step_number)`. End them after response attributes in `on_language_model_call_end`; abort closes with `cancelled`, model/stream errors with their category and duration. A retry on the same key closes the prior span with `error.type = retry`; `on_end` closes leftovers. Non-streaming spans record response attributes and close immediately on return.
- [Decision] Successful model metrics come from `on_language_model_call_end` (`performance.response_time`, `input`/output totals, and streaming first-chunk time). Failures use wrapper-measured duration plus error type. Embedding/reranking start/end pairs measure duration (embedding tokens as `input`); `on_error` records failures. `execute_tool` records duration with `gen_ai.tool.type = function`.
- [Decision] Content recording is off by default. Do not implement input/output message attributes. `OtelTelemetryBuilder::record_tool_content()` enables JSON tool arguments (only if call `record_inputs` supplied them) and results. These convention attributes are opt-in because they may contain sensitive data.
- [Decision] Resolve Tracer/Meter once at `build()`, using global providers unless explicitly supplied through `tracer_provider`/`meter_provider` (in-memory exporters in tests). `without_metrics()` emits spans only. Scope is `ferrin-otel` plus crate version. Production dependencies are the OpenTelemetry API and `tracing` context bridge; `opentelemetry_sdk` with `testing` is dev-only.
- [Fact] Modules: `telemetry.rs` (about 480 lines), `metrics.rs`, `semconv.rs`. Fourteen tests in `tests/suite/{spans,metrics}.rs` use in-memory span/metric exporters for generation/streaming/failure/abort spans, tool content opt-in and failures, `tracing` parent relationships, token/duration/first-chunk metrics, embedding/reranking and failed modality metrics, and disabled metrics.

[Decision] Telemetry step/end callbacks receive filtered copies: `record_inputs = false` removes request bodies and messages, and `record_outputs = false` removes step content, response bodies/messages and provider metadata. Model-call-end raw response bodies follow the output flag too. Application step/end hooks and returned results retain their complete values. These flags govern lifecycle event content; execution wrappers still handle the actual typed outcomes they instrument.

[Decision] Telemetry error callbacks retain error variants, status/retry classifications, attempt counts and output usage while redacting excluded request/response payloads. When either recording flag is disabled, opaque messages and nested causes are omitted because their content cannot be separated reliably; structured-output text follows `record_outputs`. Tool failure events retain a failure marker without the error payload when outputs are disabled. Application-facing errors remain unchanged.

[Decision] When either content-recording flag is disabled, telemetry warning copies retain categories and feature/setting names but omit opaque messages/details, which can contain prompt or reasoning text. Unconditional warning tracing always omits these descriptions, independent of recording flags; application warning values remain complete.

[Fact] Embedding telemetry uses a unique request `call_id` for every chunk and retry attempt. The ID retains the logical parent call prefix as `<parent>/chunk/<index>/attempt/<attempt>`; every actual request emits a matching end or error event, so concurrent chunks cannot overwrite each other in integrations (2026-09-15, core embedding and OpenTelemetry metrics regression tests).

## Runtime context recording (2026-09-17)

[Decision] Application start, step, model-call and tool-execution hooks and the end hook receive runtime state. Telemetry integration copies omit it unless `include_runtime_context` is enabled. Step snapshots independently retain tool context only with `include_tools_context`; both options default to false and are independent of input/output recording. See [ADR 0021](../04-decisions/2026-09-17-0021-agent-runtime-context.md).

## Reference callback alignment (2026-09-17)

[Decision] Embedding/reranking operation events are exposed as `on_embed_operation_start/end` and `on_rerank_operation_start/end`, distinct from attempt callbacks. Dispatch application hooks and integrations concurrently once per logical operation. Integration copies obey input/output recording and runtime-context switches; application hooks retain their complete payloads. This connects modality lifecycle hooks to the same integration boundary without conflating chunk retries with operations.

[Fact] Callback dispatch and execution-wrapper composition were compared with reference revision `6c6c221`, `packages/ai/src/telemetry/{telemetry,create-telemetry-dispatcher}.ts` and `util/merge-callbacks.ts`. Ferrin awaits all integration callbacks concurrently and uses the last registered model/tool wrapper as the outermost context. Regression cases are in `crates/ferrin-core/tests/suite/telemetry_async.rs`.

[Fact] Non-streaming generation now emits `on_error` after a failed operation, including structured-output parsing failures, matching the reference `generate-text/generate-text.ts` error boundary. Error recording restrictions remain enforced in the dispatcher; `telemetry_errors.rs` covers both generation modes.

[Decision] Tool execution context includes `record_outputs` so the OpenTelemetry wrapper can apply the call's output restriction even though it receives the actual execution result. Embedding/reranking model start/end callbacks create and finish `CLIENT` spans as well as metrics, corresponding to `packages/otel/src/open-telemetry.ts` model-call spans. Failed modality spans use the existing redacted error categories.

[Fact] This callback pass does not establish complete OpenTelemetry parity: Ferrin still lacks the reference operation/step span hierarchy, supplemental attribute groups, span enrichment callback and GenAI message formatting. Telemetry also retains explicit per-call registration, opt-in content recording and whole-context inclusion switches; the reference additionally has a global registry, input/output recording defaults and per-property context filters. These are concrete remaining differences, not verified equivalents.
