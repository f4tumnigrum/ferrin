# API 参考与示例

[English](../../02-api/02-api-reference.md) | **简体中文**

本文档列出 `ferrin` 门面 crate 暴露的公共 API 及典型用法。签名为目标形态，实现时以 rustdoc 为准；rustdoc 与本文档不一致时必须同步修订本文档。

## 1. 供应商与模型

```rust
use ferrin::openai::{create_openai, OpenAiSettings};
use ferrin::anthropic::{create_anthropic, AnthropicSettings};

let openai = create_openai(OpenAiSettings::default())?;          // reads OPENAI_API_KEY lazily
let anthropic = create_anthropic(AnthropicSettings {
    api_key: Some(SecretString::from(std::env::var("MY_ANTHROPIC_KEY")?)),
    ..Default::default()
})?;

let gpt = openai.responses("gpt-5");
let claude = anthropic.messages("claude-sonnet-4-5");
```

## 2. `generate_text`

```rust
pub fn generate_text(model: impl Into<LanguageModelRef>) -> GenerateText<()>;

impl<O> GenerateText<O> {
    // prompt
    pub fn system(self, instructions: impl Into<Instructions>) -> Self;
    pub fn prompt(self, text: impl Into<String>) -> Self;
    pub fn messages(self, messages: impl IntoIterator<Item = Message>) -> Self;
    pub fn allow_system_in_messages(self) -> Self;

    // sampling
    pub fn max_output_tokens(self, n: u32) -> Self;
    pub fn temperature(self, t: f64) -> Self;
    pub fn top_p(self, p: f64) -> Self;
    pub fn top_k(self, k: u32) -> Self;
    pub fn presence_penalty(self, p: f64) -> Self;
    pub fn frequency_penalty(self, p: f64) -> Self;
    pub fn stop_sequences(self, seqs: impl IntoIterator<Item = impl Into<String>>) -> Self;
    pub fn seed(self, seed: u64) -> Self;
    pub fn reasoning(self, effort: ReasoningEffort) -> Self;

    // tools
    pub fn tools(self, tools: ToolSet) -> Self;
    pub fn tool_choice(self, choice: ToolChoice) -> Self;
    pub fn active_tools(self, names: impl IntoIterator<Item = impl Into<ToolName>>) -> Self;
    pub fn tool_order(self, names: impl IntoIterator<Item = impl Into<ToolName>>) -> Self;
    pub fn tools_context(self, ctx: impl Serialize) -> Self;
    pub fn tool_approval(self, policy: impl ApprovalPolicy + 'static) -> Self;
    pub fn tool_approval_secret(self, secret: SecretBox<[u8]>) -> Self;
    pub fn tool_callers(self, callers: ToolCallers) -> Self;
    pub fn repair_tool_call(self, repair: impl ToolCallRepair + 'static) -> Self;
    pub fn refine_tool_input(self, name: impl Into<ToolName>, f: impl Fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, Error>> + Send + Sync + 'static) -> Self;
    pub fn sandbox(self, sandbox: Arc<dyn Sandbox>) -> Self;

    // loop control
    pub fn stop_when(self, condition: impl StopCondition + 'static) -> Self;      // may be called multiple times (any-of)
    pub fn prepare_step(self, f: impl PrepareStep + 'static) -> Self;
    pub fn output<T>(self, output: Output<T>) -> GenerateText<T>;

    // request
    pub fn max_retries(self, n: u32) -> Self;
    pub fn retry_policy(self, policy: RetryPolicy) -> Self;
    pub fn timeout(self, timeout: impl Into<Timeout>) -> Self;
    pub fn cancellation(self, token: CancellationToken) -> Self;
    pub fn headers(self, headers: Headers) -> Self;
    pub fn provider_options(self, options: ProviderOptions) -> Self;
    pub fn download(self, f: Arc<dyn DownloadFn>) -> Self;
    pub fn include(self, include: Include) -> Self;

    // observability
    pub fn telemetry(self, options: TelemetryOptions) -> Self;
    pub fn on_start(self, f: impl HookFn<StartEvent>) -> Self;
    pub fn on_step_start(self, f: impl HookFn<StepStartEvent>) -> Self;
    pub fn on_language_model_call_start(self, f: impl HookFn<ModelCallStartEvent>) -> Self;
    pub fn on_language_model_call_end(self, f: impl HookFn<ModelCallEndEvent>) -> Self;
    pub fn on_tool_execution_start(self, f: impl HookFn<ToolExecutionStartEvent>) -> Self;
    pub fn on_tool_execution_end(self, f: impl HookFn<ToolExecutionEndEvent>) -> Self;
    pub fn on_step_end(self, f: impl HookFn<StepResult>) -> Self;
    pub fn on_end(self, f: impl HookFn<EndEvent>) -> Self;
}
```

示例：多步工具循环。

```rust
use ferrin::prelude::*;

#[derive(Deserialize, JsonSchema)]
struct LookupOrder { order_id: String }

let tools = ToolSet::new().insert(
    "lookup_order",
    Tool::function::<LookupOrder>()
        .description("Look up an order by id.")
        .execute(|input: LookupOrder, _ctx| async move {
            Ok(json!({ "order_id": input.order_id, "status": "shipped" }))
        })
        .build(),
)?;

let result = ferrin::generate_text(&gpt)
    .system("You are a support agent.")
    .prompt("Where is order 4521?")
    .tools(tools)
    .stop_when(step_count(5))
    .on_step_end(|step| async move { tracing::info!(step = step.step_number, "step done") })
    .await?;

println!("{}", result.text());
for step in &result.steps {
    for call in step.tool_calls() {
        println!("called {} with {}", call.tool_name, call.input);
    }
}
```

## 3. `stream_text`

```rust
pub fn stream_text(model: impl Into<LanguageModelRef>) -> StreamText<()>;

impl<O> StreamText<O> {
    // all GenerateText methods, plus:
    pub fn transform(self, t: impl StreamTransform + 'static) -> Self;      // applied in order
    pub fn include_raw_chunks(self) -> Self;
    pub fn stream_retries(self, n: u32) -> Self;
    pub fn on_chunk(self, f: impl HookFn<StreamEvent>) -> Self;
    pub fn on_error(self, f: impl Fn(&Error) -> BoxFuture<'static, ErrorDecision> + Send + Sync + 'static) -> Self;
    pub fn on_abort(self, f: impl HookFn<AbortEvent>) -> Self;
}

impl<O> StreamTextResult<O> {
    pub fn split(self) -> (EventStream, Completion<O>);
    pub fn text_stream(self) -> TextStream;                        // Stream<Item = Result<String, Error>>
    pub fn partial_output_stream(self) -> PartialOutputStream<O>;
    pub fn element_stream(self) -> ElementStream<O::Element> where O: ArrayOutput;
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error>;
}

impl<O> Completion<O> {
    pub async fn result(self) -> Result<GenerateTextResult<O>, Error>;
}
```

示例：转发事件到 SSE，同时等待最终结果。

```rust
let stream = ferrin::stream_text(&claude)
    .prompt("Write a haiku about ownership.")
    .transform(smooth_stream(SmoothStreamConfig::default()))
    .await?;

let (mut events, completion) = stream.split();

let forward = tokio::spawn(async move {
    while let Some(event) = events.next().await {
        let line = serde_json::to_string(&event)?;
        sse_tx.send(line).await?;
    }
    Ok::<_, anyhow::Error>(())
});

let final_result = completion.result().await?;
forward.await??;
println!("total tokens: {:?}", final_result.total_usage.output.total);
```

## 4. 消息构造

```rust
let messages = vec![
    Message::system("You are a helpful assistant."),
    Message::user("Describe this image."),
    Message::user_parts([
        UserPart::text("And this document:"),
        UserPart::file_bytes(pdf_bytes, "application/pdf").with_filename("report.pdf"),
        UserPart::image_url("https://example.com/cat.png".parse()?),
    ]),
];

let result = ferrin::generate_text(&gpt).messages(messages).await?;
let mut history = messages;
history.extend(result.response_messages());
```

## 5. 工具审批

```rust
let tools = ToolSet::new().insert(
    "delete_file",
    Tool::function::<DeleteFile>()
        .description("Delete a file.")
        .needs_approval(NeedsApproval::Always)
        .execute(delete_file)
        .build(),
)?;

let first = ferrin::generate_text(&gpt)
    .prompt("Delete temp.log")
    .tools(tools.clone())
    .tool_approval_secret(secret.clone())
    .await?;

let mut history = first.response_messages();
for request in first.last_step().tool_approval_requests() {
    // ask the user; then append the response to the last tool message
    history.push_approval_response(ToolApprovalResponse::approved(request.approval_id.clone()));
}

let second = ferrin::generate_text(&gpt)
    .messages(history)
    .tools(tools)
    .tool_approval_secret(secret)
    .await?;
```

策略化审批（feature `policy`，crate `ferrin-policy`；见[策略化工具审批](../01-architecture/18-policy-approval.md)）：

```rust
use ferrin::policy::{HttpPolicyClient, policy_approval, shadow, Enforcement};

let opa = HttpPolicyClient::builder(url::Url::parse("https://policy.internal.example/")?)
    .header("authorization", "Bearer <token>")
    .build()?;
let policy = shadow(policy_approval(opa, "ferrin/tools/decision"))
    .enforcement(Enforcement::Enforce);

let result = ferrin::generate_text(&gpt)
    .prompt("Delete temp.log")
    .tools(tools)
    .tool_approval(policy)
    .await?;
```

## 6. 结构化输出

见[结构化输出](../01-architecture/08-structured-output.md)第 6 节示例。

## 7. Agent

见 [Agent](../01-architecture/09-agent.md)第 3 节示例。

## 8. 其他模态

```rust
let embeddings = ferrin::embed_many(openai.embedding("text-embedding-3-small"), texts)
    .max_parallel_calls(4)
    .await?;

let image = ferrin::generate_image(openai.image("gpt-image-1"), "A lighthouse at dawn")
    .size(ImageSize::new(1024, 1024))
    .await?;
tokio::fs::write("lighthouse.png", &image.images[0].data).await?;

let speech = ferrin::generate_speech(openai.speech("gpt-4o-mini-tts"), "Hello from Ferrin")
    .voice("alloy")
    .await?;

let transcript = ferrin::transcribe(openai.transcription("gpt-4o-transcribe"), audio /* Bytes or Url */)
    .await?;

// feature `voyage` (unreleased)
let voyage = ferrin::voyage::create_voyage(Default::default())?;
let ranked = ferrin::rerank(voyage.reranking("rerank-2.5"), "rust async", documents)
    .top_n(3)
    .await?;

let upload = ferrin::upload_file(openai.files(), pdf_bytes)
    .media_type("application/pdf")
    .filename("spec.pdf")
    .await?;
let part = UserPart::file_reference(upload.provider_reference, "application/pdf");

// feature `realtime`
let mut session = ferrin::realtime::realtime_session(openai.realtime().model("gpt-realtime")?)
    .instructions("You are a voice assistant.")
    .tools(weather_tools())
    .connect()
    .await?;
let handle = session.handle();
handle.send_text("What is the weather in Rome?").await?;
while let Some(event) = session.next_event().await {
    match event? {
        RealtimeServerEvent::TextDelta { delta, .. } => print!("{delta}"),
        RealtimeServerEvent::ResponseDone { .. } => break,
        _ => {}
    }
}
session.close().await?;
```

（2026-09-13）各函数的实际签名见[其他模态](../01-architecture/11-other-modalities.md)第 13 节：`embed_many` 接受任意 `IntoIterator<Item: Into<String>>`；`generate_image` 结果为 `GenerateImageResult { images, .. }`，`image.images[0].media_type` 非可选；`transcribe` 的音频参数为 `impl Into<AudioInput>`（`Bytes`、`Vec<u8>`、`Url`）；`rerank` 结果无 `usage`；`upload_file` 的数据参数为 `impl Into<UploadData>`。

## 9. 中间件与注册表

见[中间件与注册表](../01-architecture/10-middleware-and-registry.md)第 3 节示例。

## 10. MCP

见 [MCP 集成](../01-architecture/15-mcp.md)第 4 节示例。

## 11. 错误处理

```rust
match ferrin::generate_text(&gpt).prompt("hi").await {
    Ok(result) => println!("{}", result.text()),
    Err(err) if err.is_retryable() => tracing::warn!(%err, "transient failure"),
    Err(err) if err.status_code() == Some(StatusCode::UNAUTHORIZED) => {
        eprintln!("check OPENAI_API_KEY");
    }
    Err(Error::NoObjectGenerated(details)) => eprintln!("model returned non-conforming output: {:?}", details.text),
    Err(err) => return Err(err.into()),
}
```

## 12. 测试辅助（`ferrin-testing`）

```rust
use ferrin_testing::{MockLanguageModel, simulate_stream};

let model = MockLanguageModel::builder()
    .do_generate(|_opts| GenerateResult::text("hello"))
    .do_stream(|_opts| simulate_stream([
        StreamPart::text_start("0"),
        StreamPart::text_delta("0", "hel"),
        StreamPart::text_delta("0", "lo"),
        StreamPart::text_end("0"),
        StreamPart::finish(FinishReasonKind::Stop, Usage::default()),
    ]))
    .build();

let result = ferrin::generate_text(&model).prompt("hi").await?;
assert_eq!(result.text(), "hello");
```

`MockLanguageModel` 记录每次调用的 `CallOptions` 快照（`model.calls()`），供断言请求内容。

## 13. 稳定性标注

| 模块 | `0.y` 阶段稳定性 |
| --- | --- |
| `generate_text`、`stream_text`、消息、工具、结构化输出、错误 | 核心，变更需 ADR |
| Agent、中间件、注册表、遥测 | 核心 |
| `embed`、`generate_image`、`generate_speech`、`transcribe`、`rerank`、`upload_file` | 核心 |
| `generate_video`、批处理、实时会话、语音翻译、`stream_transcribe` | 对应的供应商 API 仍在演进；Ferrin 文档标注 `# Stability: evolving`，允许在次版本中调整 |
| `ferrin-mcp` | evolving |
| `ferrin-policy` | evolving |
| `Sandbox` | evolving |

## 14. 实现记录（2026-09-14，`ferrin` 门面）

- 【事实】crate 根 re-export `ferrin-core` 的全部公共模块（`agent`、`batch`、`embed`、`generate_text`、`stream_text`、`output`、`middleware`、`registry`、`telemetry`、`retry`、`timeout`、各模态模块等）与入口项（`generate_text`、`stream_text`、`embed`、`embed_many`、`generate_image`、`generate_speech`、`transcribe`、`rerank`、`upload_file`、`step_count`、`has_tool_call`、`Error`、`Output`、`Agent`、`ToolLoopAgent`、`TelemetryOptions` 等；`realtime` 模块与 `realtime_session` 受 feature `realtime` 控制）。下层 crate 以模块别名暴露：`ferrin::spec`、`ferrin::message`、`ferrin::schema`、`ferrin::tool`、`ferrin::provider_util`；另 re-export `serde`、`serde_json`、`schemars`（供 `#[serde(crate = "ferrin::serde")]`、`#[schemars(crate = "ferrin::schemars")]` 与 `json!` 使用）。
- 【事实】供应商 crate 既在 crate 根（`ferrin::openai`、`ferrin::anthropic`、`ferrin::google`、`ferrin::openai_compatible`，与第 1 节示例一致）也在 `ferrin::providers::*` 下（与[Crate 划分](../01-architecture/02-crates.md)第 5 节一致）以模块别名暴露；`ferrin::mcp`、`ferrin::otel` 同理，各受同名 feature 控制。`#[ferrin::tool]` 属性宏（feature `macros`，默认开）与模块 `ferrin::tool` 同名共存（宏命名空间与类型命名空间互不冲突）。
- 【决策】`ferrin::prelude` 的内容：入口函数与结果类型（上表）、`Message`/`UserPart`/`AssistantPart`/`MessagesExt`/`ToolApprovalResponse`/`Role`、`Tool`/`ToolSet`/`ToolContext`/`ToolError`/`NeedsApproval`/`Schema`/`JsonSchema`、常用规范类型（`LanguageModel`、`LanguageModelRef`、`EmbeddingModel`、`ImageModel`、`ToolChoice`、`ReasoningEffort`、`FinishReason(Kind)`、`Usage`、`JsonValue`/`JsonObject`、`Headers`、`ProviderOptions`、`ImageSize`、`ProviderError`、`StreamPart`、`GenerateResult`、`Content`）、`serde::{Deserialize, Serialize}`、`serde_json::json`、`futures_util::StreamExt`（消费 `text_stream()` 等流）与 `tool` 宏。依据：第 2 节示例只 `use ferrin::prelude::*;` 即可编译；派生宏展开引用 `serde`/`schemars` 路径，使用方需直接依赖这两个 crate 或加 `crate = "ferrin::…"` 属性（rustdoc 与 README 已说明）。
- 【事实】测试（`crates/ferrin/tests/suite/`）：prelude 下的 `generate_text`/`stream_text`/工具循环（`MockLanguageModel`）、各 feature 的供应商与扩展 re-export、`#[ferrin::tool]` 展开（异步/同步、带上下文、文档描述、参数 doc 注释进入 schema）、`trybuild` 用例（`tests/ui/pass/` 1 个、`tests/ui/fail/` 7 个：引用参数、嵌套引用、显式生命周期、泛型、无输入参数、无返回类型、宏参数）。`trybuild` 首次运行需构建独立项目，`.config/nextest.toml` 为该用例设置 180 s 的慢测试周期。

## 15. 运行状态与工具定制（2026-09-17）

【决策】生成、流式和 Agent 构建器提供 `.runtime_context(json!({...}))`，独立于 `.tools_context(...)`。`PrepareStepContext::runtime_context` 和 `ApprovalContext::runtime_context` 暴露当前应用状态；`StepOverrides` 的 `.with_runtime_context(...)` 替换当前及后续步骤的状态。`PreparedCall::runtime_context` 用于 Agent 按调用准备。`StepResult` 记录两类上下文快照；遥测仅在 `include_runtime_context` / `include_tools_context` 开启时导出相应上下文。参见 [ADR 0021](../04-decisions/2026-09-17-0021-agent-runtime-context.md)。

【决策】`Tool::into_builder()` 保留现有工具的完整定义和回调，并以 `ToolBuilder<JsonValue>` 重新开放配置；可给供应商定义的本地工具工厂附加 `.execute(...)`。解析调用及各类工具结果带有从当前定义复制的可选 `tool_metadata`。供应商路由元数据也会通过本地执行，传播到响应消息的供应商参数。

## 未发布供应商扩展（2026-09-17）

【事实】门面通过 `azure` feature 导出 `ferrin::azure`，通过 `voyage` 导出 `ferrin::voyage`；详见 [Azure](../providers/azure.md) 与 [Voyage](../providers/voyage.md)。`realtime` 同时为已启用的 OpenAI/Google 供应商开启流式音频，不会自行启用供应商。来源：`crates/ferrin/Cargo.toml`。

## 从 0.1.2 迁移到未发布开发版本

【决策】[ADR 0021](../04-decisions/2026-09-17-0021-agent-runtime-context.md) 的状态延续改变 `prepare_step` 的行为：消息、指令和两类上下文覆盖持续到后续步骤；模型、工具选择和采样覆盖仍仅影响当前步骤。只需覆盖一次的提示压缩不必重复执行。需要恢复旧行为的回调应在下一步显式恢复原始指令或上下文；恢复完整消息时使用 `initial_messages` 加 `response_messages`。

【事实】新增字段会影响下游 Rust 结构体字面量和精确枚举模式：`StepResult` 与 `StreamEvent::StartStep` 增加 `runtime_context` / `tools_context`，`ParsedToolCall`、`ToolResult`、`ToolExecutionError`、`ToolOutputDenied` 增加 `tool_metadata`；`ToolOutputDenied` 另增加 `provider_metadata`。准备步骤、审批、Agent 准备结果和生命周期事件也增加 `runtime_context`。手写字面量需显式提供新字段（未使用时填 `None`），`StartStep` 模式可使用 `..` 忽略无关字段；优先使用现有构建器/构造器。持久化结果中缺失的可选字段仍按 `None` 反序列化。来源：`crates/ferrin-core/src/generate_text/{step,prepare_step}.rs`、`stream_text/events.rs`、`agent/tool_loop_agent.rs` 与 `telemetry/events.rs`，2026-09-17。

【决策】应用回调与结果可读取上下文；Telemetry 默认不导出这两类上下文。需要导出时分别设置 `TelemetryOptions::include_runtime_context` 和 `include_tools_context`，使用 `..Default::default()` 保留其余默认设置。新供应商 feature 为可选项，不会自动启用；上述源码变更尚未发布，不代表已经做出新的版本发布决定。

### 参考行为对齐（ADR 0026）

【事实】所有 `Telemetry` 生命周期回调改为返回 `BoxFuture<'a, ()>` 并被等待。实现从同步函数体迁移为 `Box::pin(async move { ... })`；集成回调并发完成并隔离 panic。`ToolExecutionContext` 增加 `record_outputs`，包装器记录结果内容前必须遵守该设置。来源：`ferrin-core/src/telemetry/{mod,dispatcher}.rs`、`ferrin-otel/src/telemetry.rs`。

【事实】`embed`、`embed_many`、`rerank` 支持 `.runtime_context(...)`、`.on_start(...)`、`.on_end(...)`。这些 hooks 在分块重试之外，按逻辑操作触发一次；embedding 事件区分单值和多值输入/结果，空文档 rerank 成功时也触发两个事件。集成使用 `Telemetry::on_embed_operation_start/end` 和 `on_rerank_operation_start/end`，与应用 hooks 并发接收过滤后的副本。已有遥测请求尝试回调仍独立存在。来源：`ferrin-core/src/{modality_hooks,embed,rerank}.rs` 和 `telemetry/dispatcher/modalities.rs`。

【事实】Embedding 分量和 `cosine_similarity` 改用 `f64`，保留参考实现的数值精度；现有 `Vec<f32>` 需要显式转换。`Instructions` 为非穷尽枚举，包含 `System(SystemMessage)` 和 `Messages(Vec<SystemMessage>)`；文本和系统消息向量优先使用 `.into()`。空指令数组保持为空，每条消息保留其 provider options。来源：`ferrin-spec/src/embedding_model.rs`、`ferrin-core/src/{embed,prompt/standardize}.rs`。

【事实】`PrepareStepContext::model` 改为配置的模型实例，需要身份信息时调用 `provider()` / `model_id()`。准备阶段可读取初始指令和 sandbox；sandbox 覆盖仅影响当前步骤。Agent 的 `.call_options_schema(schema)` 在 `prepare_call` 前验证并归一化序列化选项，仅使用此方法时要求选项可序列化。`PreparedCall` 也支持审批设置、工具 caller、修复和细化回调、步骤准备与下载器。来源：`ferrin-core/src/agent/{options,prepare_call,tool_loop_agent}.rs`、`generate_text/prepare_step.rs`。

【决策】生命周期 hooks 并发完成并隔离同步及异步 panic；不要用回调注册顺序协调副作用。显式调用 timeout 覆盖 Agent 准备结果中的 timeout。修订后的顺序约定见 [Agent](../01-architecture/09-agent.md)。

【事实】`StreamTextResult::final_result()` 和 `split()` 返回的 completion 自行驱动管线。`full_stream`、`text_view`、`partial_output_view`、`element_view` 从当前游标创建独立视图；落后视图的事件保持缓冲，直到消费或丢弃。`into_shared_completion()` 返回可克隆的等待器，共享最终分配，不要求输出实现 `Clone`。最后一个所有者被丢弃时取消未完成工作。来源：`ferrin-core/src/stream_text/result.rs` 与 `result/tee.rs`。

【事实】`GenerateTextResult::warnings()` 返回所有步骤的警告引用，usage 也汇总所有步骤。`RerankResult::original_documents` 保留原始文档。`RequestBody::to_bytes()` 和 multipart `encode()` 改为返回 `Result`；`into_stream()` 保留一次性流所有权。来源：`ferrin-core/src/{generate_text/result,rerank}.rs`、`ferrin-provider-util/src/http/{request_body,multipart}.rs`。

【决策】OpenAI 和兼容适配器保留传入的 JSON schema，不再自动执行 `OpenAiStrict`。需要该转换时，在传入 schema 前显式执行。供应商工具工厂使用参考输入 schema，应用对象默认值并移除未声明字段；`Schema::from_json_schema` 仍只做验证。参见 [OpenAI](../providers/openai.md) 与 [OpenAI-compatible](../providers/openai-compatible.md)。

【事实】Policy 默认输入改为 `{tool: {name}, args, messages, runtimeContext}`。Shadow 观察模式允许执行，并将原始决策传给观察回调；异步观察器使用 `.on_decision(...)`，同步观察器使用 `.on_decision_sync(...)`，关闭前需要完成观察时调用 `flush_decisions().await`。来源：`ferrin-policy/src/{approval,decision,shadow}.rs`。

【事实】MCP 多轮工具输入和取消通知改为默认关闭；需要保留这些扩展时，显式设置 `McpClientConfig::max_input_rounds` 与 `send_cancel_notifications`。`initialization_timeout` 覆盖传输启动、发现和初始化。OAuth 回调通过 `AuthOptions::callback_state` / `callback_issuer` 接收参数；供应商可用新增存储 hooks 持久化 state 和授权服务器信息。令牌和客户端的过期及签发数值改用 `f64`，授权资源为可选项，存储凭据包含可选 `OAuthAuthorizationServerInformation`。已有未绑定令牌使用前会失效。来源：`ferrin-mcp/src/client/mod.rs`、`oauth/{auth,flow,provider,types}.rs`；剩余差异见 [MCP](../01-architecture/15-mcp.md)。
