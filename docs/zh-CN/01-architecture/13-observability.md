# 可观测性

[English](../../01-architecture/13-observability.md) | **简体中文**

位于 `ferrin-core::telemetry`，OpenTelemetry 导出位于 `ferrin-otel`。

## 1. 遥测集成接口

【决策】`Telemetry` 是一组可选生命周期回调（开始、步骤开始/结束、模型调用开始/结束、工具执行开始/结束、嵌入与重排开始/结束、结束、中止、错误），以及两个上下文包装函数 `execute_language_model_call` 与 `execute_tool`（用于在集成自己的 span 上下文中运行调用）。`TelemetryOptions` 包含启用开关、是否记录输入/输出、`function_id`、`metadata`、是否包含运行时上下文与工具上下文，以及集成列表。依据：回调覆盖生成循环的全部阶段边界，包装函数则是 OpenTelemetry 一类需要父子 span 上下文的集成所必需的。

```rust
pub trait Telemetry: Send + Sync + 'static {
    fn on_start(&self, event: &StartEvent) {}
    fn on_step_start(&self, event: &StepStartEvent) {}
    fn on_language_model_call_start(&self, event: &ModelCallStartEvent) {}
    fn on_language_model_call_end(&self, event: &ModelCallEndEvent) {}
    fn on_tool_execution_start(&self, event: &ToolExecutionStartEvent) {}
    fn on_tool_execution_end(&self, event: &ToolExecutionEndEvent) {}
    fn on_step_end(&self, event: &StepEndEvent) {}
    fn on_embed_start(&self, event: &EmbedStartEvent) {}
    fn on_embed_end(&self, event: &EmbedEndEvent) {}
    fn on_rerank_start(&self, event: &RerankStartEvent) {}
    fn on_rerank_end(&self, event: &RerankEndEvent) {}
    fn on_end(&self, event: &EndEvent) {}
    fn on_abort(&self, event: &AbortEvent) {}
    fn on_error(&self, event: &ErrorEvent) {}

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

【决策】回调为同步方法（不返回 Future）。依据：回调在管线热路径上执行，异步回调会把外部延迟引入流处理；需要异步处理的集成应在回调内把事件投递到自己的通道。`execute_*` 包装函数保留异步形态，因为它们必须包裹实际调用。

【决策】不提供全局遥测注册表。集成通过 `TelemetryOptions::integrations` 按调用、按 Agent 或按注册表中间件注入。依据：与“无全局可变状态”原则一致；需要进程级默认的应用可在自己的构建器封装中固定 `TelemetryOptions`。

## 2. 事件载荷

事件结构体字段（节选）：

| 事件 | 字段 |
| --- | --- |
| `StartEvent` | `call_id`、`function_id`、`model: ModelIdentity {provider, model_id}`、`inputs: Option<RecordedInputs>`（受 `record_inputs` 控制）、`metadata` |
| `StepStartEvent` | `call_id`、`step_number`、`model`、`messages: Option<Arc<[Message]>>` |
| `ModelCallStartEvent` | `call_id`、`step_number`、`call_options_snapshot`（可序列化投影） |
| `ModelCallEndEvent` | `content`、`finish_reason`、`usage`、`response`、`performance`、`warnings` |
| `ToolExecutionStartEvent` | `tool_call_id`、`tool_name`、`input: Option<JsonValue>` |
| `ToolExecutionEndEvent` | `tool_call_id`、`tool_name`、`output: Option<ToolOutcome>`、`duration` |
| `StepEndEvent` | `step: Arc<StepResult>` |
| `EndEvent` | `steps`、`total_usage`、`output_recorded: Option<JsonValue>` |
| `AbortEvent` | `call_id`、`steps_completed` |
| `ErrorEvent` | `call_id`、`error: &Error`、`phase: ErrorPhase` |

## 3. 内置 tracing

【决策】核心层始终通过 `tracing` crate 创建 span 与事件，独立于 `Telemetry` 集成。span 命名与字段遵循 OpenTelemetry GenAI 语义约定（`gen_ai.*`），使应用只需接入 `tracing-subscriber` 或 `tracing-opentelemetry` 即可获得基本可观测性。依据：`tracing` 是 Rust 生态的事实标准；在函数定义处以 `#[tracing::instrument]` 埋点是编码规范的要求。

| span | 触发位置 | 关键字段 |
| --- | --- | --- |
| `ferrin.generate_text` / `ferrin.stream_text` | 调用入口 | `gen_ai.operation.name`、`ferrin.function_id`、`gen_ai.request.model`、`gen_ai.provider.name` |
| `ferrin.step` | 每步 | `ferrin.step_number` |
| `ferrin.model_call` | `do_generate`/`do_stream` | `gen_ai.request.*`（采样参数）、`gen_ai.response.id`、`gen_ai.response.finish_reasons`、`gen_ai.usage.input_tokens`、`gen_ai.usage.output_tokens`、`ferrin.time_to_first_output_ms` |
| `ferrin.tool` | 工具执行 | `gen_ai.tool.name`、`gen_ai.tool.call.id`、`ferrin.tool.duration_ms` |
| `ferrin.modality`（2026-09-13 修订，原为 `ferrin.embed` / `ferrin.rerank` / `ferrin.image` 等；见第 8 节与 [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 5 项） | 其他模态 | `gen_ai.operation.name`、`gen_ai.request.model`、`gen_ai.provider.name` |

输入与输出内容只在 `record_inputs`/`record_outputs` 为真时以 `tracing` 事件记录（`target = "ferrin::telemetry::content"`），便于订阅方按 target 过滤。

## 4. 警告日志

【事实】适配器产生的警告（不支持的选项、兼容映射、弃用）需要让开发者看到，但库不应直接写控制台。

【决策】警告以 `tracing::warn!(target: "ferrin::warnings", ...)` 记录，每条警告一个事件，字段为 `warning.type`、`warning.feature`、`warning.details`、`provider`、`model_id`。应用通过 tracing 过滤器控制；不设全局开关。

## 5. 性能指标

【决策】步骤结果包含性能指标：响应耗时、首个输出耗时、输出令牌速率（以及把首块延迟计入的有效速率）、输入令牌速率、总令牌有效速率，以及输出分片间隔的统计（最小、最大、均值、p50、p90、p99、计数）。依据：这些指标不需要额外请求即可从时间戳与用量计算，是比较供应商与模型的基本依据。

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

`ferrin_otel::OtelTelemetry` 实现 `Telemetry`（实现细节见第 9 节）：

- 在 `execute_language_model_call` 与 `execute_tool` 中创建子 span：父上下文取当前 `tracing` span 经 `tracing-opentelemetry` 0.33.0 暴露的 OTel 上下文（安装了该 layer 时），否则取 `opentelemetry::Context::current()`；被包裹的调用在新 span 的上下文中运行（`FutureExt::with_context`）。
- 属性命名遵循 GenAI 语义约定；Ferrin 特有属性使用 `ferrin.*` 前缀（常量见 `ferrin_otel::semconv`）。
- 用量与耗时以 OTel 直方图 `gen_ai.client.token.usage`、`gen_ai.client.operation.duration`、`gen_ai.client.operation.time_to_first_chunk` 与 `gen_ai.execute_tool.duration` 导出。

【事实】（PV-014，`verification/pv014-otel`）`opentelemetry` 0.32.0 + `opentelemetry_sdk` 0.32.1 + `tracing-opentelemetry` 0.33.0 可共同编译并记录 span。`opentelemetry-semantic-conventions` 0.32.1 中全部 `GEN_AI_*` 常量已标记 `#[deprecated]`（注释：已迁移至 OpenTelemetry GenAI 语义约定仓库）。

【决策】`ferrin-otel` 在 `semconv.rs` 中自行定义 GenAI 属性名常量（`gen_ai.operation.name`、`gen_ai.provider.name`、`gen_ai.request.model`、`gen_ai.usage.input_tokens` 等），不使用 `opentelemetry-semantic-conventions` 的已弃用常量；该 crate 因此从 `ferrin-otel` 的依赖中移除。

## 7. 示例

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

## 8. 实现记录（2026-09-13，ferrin-core）

- 【决策】（[ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 5 项）非文本模态共用 `ferrin.modality` span，`gen_ai.operation.name` 取 `embed`、`image`、`speech`、`transcription`、`rerank`、`video`、`upload_file`、`get_file_metadata`、`download_file`、`delete_file`、`upload_skill`、`start_batch`、`get_batch_status`、`get_batch_results`、`cancel_batch`、`list_batches`。
- 【决策】`ferrin.model_call` span 按重试尝试创建：每次 `do_generate`/`do_stream` 尝试一个 span，重试次数体现在 span 数量与 `Telemetry::on_language_model_call_*` 事件次数上。依据：单个跨尝试的 span 无法区分各次尝试的耗时与错误。
- 【事实】`TelemetryDispatcher` 在模态路径上分发 `on_embed_start/on_embed_end`、`on_rerank_start/on_rerank_end`（事件含 `call_id`、`ModelIdentity`、数量与耗时；输入内容仅在 `record_inputs` 为真时填充）与 `on_error(ErrorEvent { call_id, error, phase: ErrorPhase::{Prompt, ModelCall, ToolExecution, Output, Stream} })`。
- 【事实】警告日志（第 4 节）由 `spans::log_warnings(&[Warning], &ModelIdentity)` 统一输出，非文本模态每次调用后调用一次。

## 9. 实现记录（2026-09-14，ferrin-otel）

- 【事实】GenAI 语义约定已从 `opentelemetry.io` 迁移到仓库 `open-telemetry/semantic-conventions-genai`（`docs/gen-ai/gen-ai-spans.md`、`docs/gen-ai/gen-ai-metrics.md`，2026-09-14 读取，状态均为 Development）。推理 span：名称 SHOULD 为 `{gen_ai.operation.name} {gen_ai.request.model}`，kind SHOULD 为 `CLIENT`；必填 `gen_ai.operation.name`、`gen_ai.provider.name`，条件必填 `gen_ai.request.model`、`error.type`（出错时），推荐 `gen_ai.response.id`、`gen_ai.response.model`、`gen_ai.response.finish_reasons`（string[]）、`gen_ai.usage.input_tokens`、`gen_ai.usage.output_tokens`；`gen_ai.input.messages`/`gen_ai.output.messages` 为 Opt-In。工具 span：名称 `execute_tool {gen_ai.tool.name}`，kind `INTERNAL`，必填 `gen_ai.operation.name = execute_tool`、`gen_ai.tool.name`，推荐 `gen_ai.tool.call.id`、`gen_ai.tool.type`（`function`/`extension`/`datastore`）、`gen_ai.tool.description`，Opt-In `gen_ai.tool.call.arguments`、`gen_ai.tool.call.result`（span 上可记为 JSON 字符串）。指标：`gen_ai.client.token.usage`（直方图，单位 `{token}`，属性 `gen_ai.token.type` ∈ {`input`,`output`}，推荐桶 1、4、16、…、67108864）、`gen_ai.client.operation.duration`（`s`，属性含 `error.type`）、`gen_ai.client.operation.time_to_first_chunk`（`s`，仅流式）、`gen_ai.execute_tool.duration`（`s`，属性 `gen_ai.tool.name`、`gen_ai.tool.type`、`error.type`）；时长类推荐桶 0.01、0.02、0.04、…、81.92。`gen_ai.operation.name` 的已知值含 `chat`、`generate_content`、`embeddings`、`execute_tool`、`invoke_agent` 等，允许自定义值。
- 【决策】语言模型调用统一记为 `gen_ai.operation.name = chat`（Ferrin 的 `do_generate`/`do_stream` 不区分 chat 与 generate_content 形态），span 名 `chat {model_id}`；嵌入记为 `embeddings`，重排记为自定义值 `rerank`。`error.type` 与 span 状态描述取 `ferrin_core::ErrorKind::as_str()`（`provider`、`timeout`、`cancelled`、…）或 `ToolError` 变体名（`message`、`json`、`timeout`、`cancelled`），从不写入错误消息。
- 【决策】流式模型调用的 span 在 `execute_language_model_call` 返回后保持打开（附加 `ferrin.streaming = true`），以 `(call_id, step_number)` 为键挂起，`on_language_model_call_end` 写入响应属性后结束；`on_abort` 以 `cancelled`、`on_error`（阶段 `ModelCall`/`Stream`）以错误类别结束挂起 span 并记录 `gen_ai.client.operation.duration`；同一键上新的尝试（流错误重试）把上一 span 以 `error.type = retry` 结束；`on_end` 结束残留 span。非流式调用在返回时直接写入响应属性并结束。
- 【决策】指标来源：成功的模型调用在 `on_language_model_call_end` 记录（耗时取 `performance.response_time`，令牌取 `usage.input.total`/`usage.output.total`，流式时另记 `time_to_first_chunk`）；失败在 `execute_language_model_call` 内以集成自测的耗时记录并附 `error.type`；嵌入/重排以 `on_*_start`/`on_*_end` 配对记录耗时（嵌入令牌记为 `input`），`on_error` 记录失败耗时；工具在 `execute_tool` 内记录 `gen_ai.execute_tool.duration`（`gen_ai.tool.type = function`）。
- 【决策】内容默认不记录：不实现 `gen_ai.input.messages`/`gen_ai.output.messages`；`OtelTelemetryBuilder::record_tool_content()` 开启后在工具 span 上写 `gen_ai.tool.call.arguments`（仅当调用的 `record_inputs` 为真，因为核心层只在此时传入输入）与 `gen_ai.tool.call.result`（JSON 字符串）。依据：约定把这些属性列为 Opt-In 并警告含敏感数据。
- 【决策】Tracer 与 Meter 在 `build()` 时解析一次：默认取 `opentelemetry::global` 的提供者，`OtelTelemetryBuilder::tracer_provider`/`meter_provider` 可显式指定（测试用内存导出器），`without_metrics()` 只产生 span。instrumentation scope 为 `ferrin-otel` + crate 版本。库只依赖 `opentelemetry` API 与 `tracing-opentelemetry`（取当前 `tracing` span 的上下文）；`opentelemetry_sdk`（feature `testing`）仅为开发依赖。
- 【事实】模块：`telemetry.rs`（约 480 行）、`metrics.rs`、`semconv.rs`；测试 14 个（`tests/suite/{spans,metrics}.rs`，`InMemorySpanExporter` + `InMemoryMetricExporter`）：非流式/流式/失败/中止/流错误的 span 与属性、工具 span（默认无内容、开启后有内容、失败）、与 `tracing` span 的父子关系、用量/耗时/首块指标、嵌入与重排指标、失败模态指标、关闭指标。
