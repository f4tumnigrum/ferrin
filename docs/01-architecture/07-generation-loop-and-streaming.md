# 生成循环与流式管线

本文档定义 `ferrin-core::generate_text` 与 `ferrin-core::stream_text` 的行为。两者共享步骤语义、工具执行与停止条件，区别只在模型调用方式与结果交付形态。

## 1. 步骤模型

【决策】一次 `generate_text` 调用由若干步骤组成，每个步骤是一次模型调用及其后的客户端工具执行。步骤结果 `StepResult` 包含：`content`（有序内容部件）、文本、推理、文件、来源、工具调用（静态/动态）、工具结果、工具错误、完成原因（统一值与原始值）、用量、警告、请求、响应（`id`、时间戳、模型 ID、响应头、响应体、消息）、供应商元数据、性能指标、步骤序号与运行时上下文。依据：步骤是工具循环的自然单位，应用按步骤观察进度、做审计与计费。

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

`StepContent` 是核心层内容枚举，在规范层 `Content` 之上增加 `ToolCall(ParsedToolCall)`（已解析、已校验输入）、`ToolResult`、`ToolError`、`ToolApprovalRequest`、`ToolOutputDenied`。

## 2. 非流式循环

【决策】`generate_text` 主循环的每一步：

1. 调用 `prepare_step`（入参为模型、已完成步骤、步骤序号、消息与运行时上下文），允许覆盖 `model`、`tool_choice`、`active_tools`、`system`、`messages`、`runtime_context` 以及采样参数；未覆盖的项沿用外层。
2. 将当前消息转换为规范层 Prompt（下载 URL），准备工具与 `tool_choice`。
3. 在重试策略内调用 `do_generate`，同时应用步骤超时。
4. 解析每个工具调用（含修复、无效标记、`tool_choice` 违规）；`on_language_model_call_end` 回调在解析后、执行前触发。
5. 对每个客户端工具调用解析审批状态；`not-applicable`/`approved` 且完成原因允许执行时并发执行（允许执行的完成原因仅 `stop` 与 `tool-calls`）；`user-approval` 产生审批请求；`denied` 产生拒绝结果。
6. 记录供应商执行工具的延迟结果（支持延迟结果且本步未返回结果的调用加入待定列表；后续步骤返回结果时移除）。
7. 组装 `StepResult`，触发 `on_step_end`，把响应消息追加到消息序列。
8. 循环继续条件：

```typescript
} while (
  clientToolOutputs.length + deniedToolApprovalResponses.length === clientToolCalls.length &&
  (clientToolCalls.length > 0 || pendingDeferredToolCalls.size > 0) &&
  !(await isStopConditionMet({ stopConditions, steps }))
);
```

即：所有客户端工具调用都有了输出或拒绝（无待审批项、无缺少 `execute` 的工具），且存在客户端工具调用或有待补齐的延迟结果，且停止条件未满足。

9. 循环结束后：合计各步用量为 `total_usage`；若配置了 `output`，在最后一步完成原因为 `stop`、或（完成原因不是 `tool-calls` 且文本非空）时解析结构化输出（`Output::parse_complete`），否则不解析；调用 `on_end` 回调。
10. `stop_when` 默认为 `step_count(1)`；多个条件任一满足即停。内置条件：步数上限、出现指定工具的调用、循环自然结束。

【决策】Ferrin 的 `generate_text::run` 逐条实现上述规则；继续条件以同名函数 `should_continue(&LoopState) -> bool` 表达并配套单元测试覆盖四类边界（待审批、缺少执行函数、延迟结果未回、停止条件满足）。

```rust
pub trait StopCondition: Send + Sync {
    fn is_met(&self, steps: &[StepResult]) -> BoxFuture<'_, bool>;
}

pub fn step_count(n: u32) -> impl StopCondition;
pub fn has_tool_call(name: impl Into<ToolName>) -> impl StopCondition;
pub fn loop_finished() -> impl StopCondition;   // finish reason != tool-calls or no pending client tools
```

### 2.1 重试

【决策】重试策略：默认最多重试 2 次、初始延迟 2000 ms、倍率 2；错误可重试性由 `ApiCallError::is_retryable` 决定；若响应头含 `retry-after-ms` 或 `retry-after`（秒或 HTTP 日期），且换算后在 0 到 60 s 之间，使用该值替代退避延迟；重试耗尽后返回 `RetryError`（原因为次数耗尽、错误不可重试或已取消，并附全部错误）；取消时立即中止。依据：2 次重试覆盖绝大多数瞬时故障而不把总延迟推高到分钟级；尊重 `retry-after` 是各供应商限流文档的要求，60 s 上限防止服务端异常值让调用长时间挂起。

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

【决策】默认无抖动，便于测试确定性；`Jitter::Full` 作为可选项。重试只包裹单次模型调用，不包裹工具执行。

### 2.2 超时

【决策】超时配置分为总超时、步骤超时、首块超时、块间超时、工具超时与按工具名的超时；首块与块间超时仅流式生效；总超时覆盖整个调用（含工具执行）。超时以派生自调用方取消令牌的定时取消实现。依据：不同层级的超时对应不同的故障模式（供应商无响应、流中断、工具挂起），单一总超时无法区分。

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

超时触发产生 `Error::Timeout { scope: Total | Step | FirstChunk | Chunk | Tool(name) }`，取消令牌派生关系见[并发、取消与超时](16-concurrency-and-cancellation.md)。

### 2.3 结果

【决策】`GenerateTextResult` 字段：内容、文本、推理、文件、来源、工具调用、工具结果、工具错误、完成原因、最后一步用量、总用量、警告、请求、响应（含响应消息）、供应商元数据、全部步骤，以及由泛型 `O` 在编译期决定的结构化输出。

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

【决策】`output` 以泛型而非运行时可选字段表达：未配置 `Output` 时类型为 `()`，配置后为 `T`。依据：把“未指定输出却访问 output”的错误从运行时移到编译期。

### 2.4 `include` 选项

【决策】`Include { request_body, request_messages, response_body }` 控制步骤结果是否保留请求体、请求消息、响应体，默认均为 `false`；流式版本以 `raw_chunks` 替代 `response_body`。依据：这些数据体积大且可能含敏感内容，只在调试或审计时按需开启。

## 3. 流式管线

### 3.1 阶段

【决策】`stream_text` 的管线：模型流经工具执行变换、可拼接流（多步骤）、弹性流（错误处理与流级重试）、停止门、用户变换、输出变换，最后进入事件处理器。依据：把每个关注点做成独立的流变换阶段，使重试、停止与用户变换可以单独测试并按需组合。

【决策】Ferrin 的管线阶段与此对应，但结果交付改为单一事件流加完成句柄：

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

依据：Rust 的 `Stream` 是拉取式且单消费者；tee 出多个消费视图并自动消费的形态需要无界缓冲与隐式驱动，与背压和显式所有权冲突。`split` 让应用在一个任务里转发事件、在另一个任务里等待最终结果；需要多路消费的应用可自行用 `tokio::sync::broadcast` 分发。

### 3.2 事件类型

【决策】`StreamEvent` 变体：`start`、`start-step {request, warnings}`、`text-start/text-delta/text-end`、`reasoning-start/delta/end`、`reasoning-file`、`file`、`source`、`custom`、`tool-input-start/delta/end`、`tool-call`、`tool-result`（含 `preliminary`）、`tool-error`、`tool-approval-request`、`tool-output-denied`、`finish-step {finish_reason, raw_finish_reason, usage, response, provider_metadata}`、`finish {finish_reason, total_usage}`、`error`、`abort`、`raw`。

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

所有事件可序列化，应用可直接以 SSE 或 WebSocket 帧转发。`StreamErrorInfo` 是 `Error` 的可序列化投影（错误种类、消息、可重试性、状态码）。

### 3.3 工具执行阶段

【决策】流式工具执行阶段：所有分片立即前传；遇到有效、有 `execute`、非供应商执行且审批状态为 `not-applicable` 的工具调用时加入待执行列表；需要审批的产生审批请求事件；在收到 `model-call-end`（完成原因允许执行）后并发启动执行，结果与错误作为 `tool-result`/`tool-error` 事件进入流，附 `tool-execution-end {tool_call_id, tool_execution_ms}`；重试边界（`AttemptBoundary`）出现时清空待执行列表（本次尝试作废）。依据：先前传再执行使文本增量不被工具执行阻塞；以 `model-call-end` 为执行起点保证完成原因已知。

【决策】Ferrin 的该阶段用 `JoinSet` 启动工具任务，通过有界 `mpsc` 通道把结果注入流；任务持有 `Arc<Tool>` 与克隆的 `ToolContext`，取消令牌取消时所有任务中止。

### 3.4 弹性阶段与流级重试

【决策】流级重试语义：`on_error` 回调可要求重试；`stream_retries` 配置流开始后收到供应商错误时自动重试当前步骤的次数，默认禁用；重试只重跑当前步骤，之前步骤保留；已发出的部分输出不撤回，但从恢复后的步骤结果、结构化输出解析、响应消息与后续步骤中排除。依据：已经交付给消费者的事件无法收回，只能保证最终结果不重复计入。

【决策】Ferrin 提供 `stream_retries(u32)` 与 `on_error(fn) -> ErrorDecision::{Continue, Retry}`；重试边界在内部以 `AttemptBoundary` 标记，事件处理器据此丢弃上一尝试的累积内容。默认禁用：流级重试会重放已消费的部分输出，应用必须显式选择。

### 3.5 部件 ID 重映射

【事实】供应商分配的文本/推理部件 ID 仅在单次调用内唯一（Anthropic 使用内容块索引）。【决策】多步骤流中核心层重映射冲突 ID。

Ferrin 在拼接阶段维护已用 ID 集合，冲突时追加 `-<n>` 后缀。

### 3.6 用户变换

【决策】`transform` 接受一个或多个流变换，按顺序应用，必须维持事件结构；内置 `smooth_stream`（延迟默认 10 ms，切分方式为词、行、正则、Unicode 分段或自定义检测器）缓冲文本/推理增量并按词或行切分输出，`provider_metadata` 在类型或 ID 切换时随缓冲刷出。依据：供应商分片边界不稳定，按词或行重新切分让界面输出平滑。

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

【决策】`Intl.Segmenter` 的对应物为 `unicode-segmentation` 的词边界；不引入 ICU 数据。

### 3.7 事件处理器

事件处理器负责：按步骤累积内容、生成 `StepResult`、维护部件 ID、计算性能指标（首块延迟、块间间隔统计、输出 token 速率）、合计用量、在 `Finish` 后向 `Completion` 发送最终结果、调用 `on_step_end`/`on_end`/`on_abort`、向遥测分发事件。它是唯一持有可变累积状态的阶段。

### 3.8 启动语义

【决策】`stream_text(...).await` 在首步骤的模型请求建立成功（收到响应头且状态可接受，或重试耗尽失败）后返回 `Result<StreamTextResult, Error>`；后续步骤的错误通过 `StreamEvent::Error` 与 `Completion` 传递。依据：Rust 调用方期望配置与鉴权错误能在调用点用 `?` 处理；这与 `reqwest::RequestBuilder::send().await?` 后再流式读取响应体的惯例一致。被否决的备选是入口同步返回结果对象、模型请求在后台开始、请求建立错误进入流并只触发 `on_error`，见 [ADR 0005](../04-decisions/2026-09-13-0005-stream-result-delivery.md)。

## 4. 回调

【决策】回调集合：`on_start`、`on_step_start`、`on_language_model_call_start`、`on_language_model_call_end`、`on_tool_execution_start`、`on_tool_execution_end`、`on_step_end`、`on_end`；流式额外 `on_chunk`、`on_error`、`on_abort`。

【决策】Ferrin 以 `Hooks` 结构体承载 `Arc<dyn Fn(...) -> BoxFuture<'_, ()>>`，构建器方法 `on_step_end(|step| async move {...})`。回调返回 Future 且核心等待其完成（回调未完成时流处理暂停）。

## 5. 待验证

- 【事实】（PV-006，`verification/pv006-joinset`，release 构建，macOS arm64）200 个并行工具任务（1–20 ms 延迟、4 KiB 结果）经 `JoinSet` + 有界 `mpsc` 注入慢消费者：容量 1/64/1024 的耗时（284–308 ms）与峰值 RSS（2.9 MiB）无可测差异；1000 个任务时同样（1.40–1.42 s，6.7 MiB）。结果进入流的顺序为完成顺序（相对派发顺序约 30% 逆序）。通道容量不影响内存：结果在任务内产生后等待发送，内存由并行工具数决定。
- 【决策】通道容量默认 64 保持不变；内存上限通过 `ToolExecutionOptions::max_concurrency`（默认不限）控制，而非通道容量。
- 【决策】（PV-007）不增加 `start_eager()`。`stream_text(...).await` 在首步骤的 `model.stream()` 返回后完成；在 `simulate_streaming` 下该点位于完整 `generate()` 之后，等待时长等于一次非流式调用，属于该中间件的固有语义并在其文档注释中说明。需要立即获得句柄的应用可在自身任务中启动调用；库内不为此引入第二种启动语义。

## 6. 实现记录（2026-09-13）

- 【事实】`StepResult` 增加 `model: ModelIdentity { provider, model_id }`（本步实际调用的模型，`prepare_step` 切换模型时与调用入口不同），`request: StepRequest { body, messages }`，`response: StepResponse { id, timestamp, model_id, headers, body, messages }`；`ToolResult` 增加 `execution_ms: Option<u64>`（仅客户端执行的工具）。
- 【事实】`StepContent` 在第 1 节列出的变体之外还有 `ToolApprovalResponse`（重放审批时记录判定）、`Reasoning`、`ReasoningFile`、`File`、`Custom`、`Source`。
- 【决策】`Include::default()` 全部为 `false`（`Include::none()`）；`request_messages` 为真时 `StepRequest.messages` 保存发送给模型的 `Vec<Message>`。
- 【决策】审批重放产生的工具消息（`replay_tool_message`：执行通过的调用结果与被拒绝调用的 `execution-denied` 输出）位于本次调用响应消息的最前，随后才是各步骤的助手/工具消息。依据：重放结果在时间上先于首个模型调用，`response_messages()` 追加到历史后保持因果顺序。
- 【决策】`retry()` 包裹返回核心 `Error` 的操作：只有 `Error::Provider` 参与重试判定，`Error::Cancelled` 立即中止，其他核心错误原样返回；`retry_with()` 允许调用方扩展“可重试”的判定（图像生成把空结果标记视为可重试）。
- 【决策】`tool_choice` 要求调用某工具而模型未调用时，步骤以 `Error::ToolChoiceNotSatisfied { expected: Option<ToolName> }` 结束（流式路径在事件处理器中判定）。依据：显式错误让应用能区分“模型拒绝调用工具”与正常完成，静默结束会掩盖这一差异。
- 【事实】`StreamEvent` 在第 3.2 节的变体之外增加 `RetryAttempt { step_number, attempt, request, warnings }`（流级重试边界，前一尝试的内容留在流中但不计入步骤结果）与 `ToolApprovalResponse`；`ToolInputStart` 携带 `provider_executed`、`dynamic` 与 `provider_metadata`。
- 【决策】流式管线中工具输入部件（`ToolInputStart`/`Delta`/`End`）在缓冲阶段按 `id` 累积增量，`ToolInputEnd` 到达时解析并校验为 `ToolCall(ParsedToolCall)` 事件；供应商直接发出的 `tool-call` 分片走相同解析路径。依据：与非流式路径共用 `parse_tool_call`，保证修复与精炼逻辑只有一份。
- 【决策】`TransformContext::stop()` 取消本次调用：模型流与待执行工具被取消，调用以 `Error::Cancelled` 结束，管线以内部 `stop` 令牌门控后续事件；变换应同时结束自己的输出流。依据：只终止流而不取消模型请求会泄漏后台任务，并让供应商继续生成计费的输出。
- 【决策】`stream_text(...).await` 的就绪条件是首步骤的 `do_stream()` 返回（`Ok` 或经重试后的 `Err`），与第 3.8 节一致；此后首个 `StreamStart` 之前的错误通过 `StreamEvent::Error` 与 `Completion` 传递。
- 【事实】`ferrin_core::clock::Clock`（`fn now(&self) -> DateTime<Utc>`，为 `Fn() -> DateTime<Utc>` 提供 blanket impl）注入步骤时间戳与性能指标的时钟，测试用固定时钟消除快照中的时间差异。
