# Agent

Agent 抽象位于 `ferrin-core::agent`。

## 1. Agent trait

【决策】`Agent` 是把模型、工具与设置封装为可重复调用对象的接口：暴露 `id`、`tools`、`generate(...)` 与 `stream(...)`；调用参数包含 `prompt | messages`、可选类型化 `options`、取消令牌、`timeout`、生命周期回调与沙箱；流式额外接受流变换。依据：应用通常以固定配置反复调用同一组模型与工具，Agent 让这些配置只声明一次。

```rust
pub trait Agent: Send + Sync {
    type Options: Send + 'static;                       // () when the agent takes no options (2026-09-13: no DeserializeOwned bound, see §6)
    type Output: Send + 'static;                        // () when no structured output

    fn id(&self) -> Option<&str>;
    fn tools(&self) -> &ToolSet;

    fn generate(&self, call: AgentCall<Self::Options>)
        -> impl Future<Output = Result<GenerateTextResult<Self::Output>, Error>> + Send;

    fn stream(&self, call: AgentStreamCall<Self::Options>)
        -> impl Future<Output = Result<StreamTextResult<Self::Output>, Error>> + Send;
}

pub struct AgentCall<O> {
    pub input: AgentInput,            // Prompt(String) | Messages(Vec<Message>)
    pub options: O,
    pub cancellation: CancellationToken,
    pub timeout: Option<Timeout>,
    pub hooks: Hooks,
    pub sandbox: Option<Arc<dyn Sandbox>>,
}
```

【决策】不设 `version` 字段。依据：见 [Provider 规范层](04-provider-spec.md) 第 1 节，版本由 crate 版本表达。

## 2. ToolLoopAgent

【决策】`ToolLoopAgent` 的行为：

- 设置项包含 `model`、`instructions`（系统提示）、`allow_system_in_messages`、`tools`、`tool_choice`、`stop_when`（默认 `step_count(20)`）、`telemetry`、`active_tools`、`tool_order`、`output`、`runtime_context`、`tool_approval`、`tool_callers`、`tool_approval_secret`、`prepare_step`、`repair_tool_call`、`refine_tool_input`、全部生命周期回调、`provider_options`、`download`、`include`、`prepare_call`，以及全部采样参数与请求选项。
- `prepare_call` 允许根据调用选项生成模板化设置（例如按用户语言改写 `instructions`）；返回值中显式清除的字段会移除外层设置。
- 调用时 Agent 级回调与调用级回调顺序合并（Agent 级先执行）。
- 请求头追加 User-Agent 后缀 `ferrin-agent/tool-loop`。
- `generate` 与 `stream` 分别委托给 `generate_text` 与 `stream_text`。

```rust
pub struct ToolLoopAgent<Opt = (), Out = ()> { settings: Arc<ToolLoopAgentSettings<Opt, Out>> }

impl ToolLoopAgent {
    pub fn builder(model: impl Into<LanguageModelRef>) -> ToolLoopAgentBuilder<(), ()>;
}

impl<Opt, Out> ToolLoopAgentBuilder<Opt, Out> {
    pub fn id(self, id: impl Into<String>) -> Self;
    pub fn instructions(self, instructions: impl Into<Instructions>) -> Self;
    pub fn tools(self, tools: ToolSet) -> Self;
    pub fn stop_when(self, condition: impl StopCondition + 'static) -> Self;
    pub fn output<T>(self, output: Output<T>) -> ToolLoopAgentBuilder<Opt, T>;
    pub fn call_options<O: Send + 'static>(self) -> ToolLoopAgentBuilder<O, Out>;   // 2026-09-13: bounds relaxed, see §6
    pub fn prepare_call(self, f: impl Fn(PrepareCallInput<Opt>) -> BoxFuture<'static, Result<PreparedCall, Error>> + Send + Sync + 'static) -> Self;
    pub fn prepare_step(self, f: impl PrepareStep + 'static) -> Self;
    pub fn tool_approval(self, policy: impl ApprovalPolicy + 'static) -> Self;
    pub fn tool_approval_secret(self, secret: SecretBox<[u8]>) -> Self;
    pub fn telemetry(self, options: TelemetryOptions) -> Self;
    // sampling parameters, request options, hooks, provider_options, include, download ...
    pub fn build(self) -> ToolLoopAgent<Opt, Out>;
}
```

【决策】调用选项通过泛型 `Opt` 表达并要求 `DeserializeOwned + JsonSchema`，`call_options::<O>()` 同时充当选项 schema。依据：原设计假定选项来自运行期传入的 JSON 而需要 schema 校验，Rust 中类型即 schema，校验发生在反序列化。（2026-09-13 修订：取消这两个约束，见第 6 节与 [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 1 项。）

【决策】User-Agent 后缀为 `ferrin-agent/tool-loop`，与核心层 `ferrin/<version>` 及适配器 `ferrin-<provider>/<version>` 链式拼接。依据：供应商侧按 User-Agent 统计客户端来源，链式后缀同时标识 SDK、适配器与调用形态。

## 3. 示例

```rust
#[derive(Deserialize, JsonSchema)]
struct SupportOptions { customer_tier: String }

let agent = ToolLoopAgent::builder(&model)
    .id("support-agent")
    .instructions("You are a support agent. Use tools to look up orders before answering.")
    .tools(support_tools())
    .call_options::<SupportOptions>()
    .prepare_call(|input| Box::pin(async move {
        let mut prepared = input.defaults;
        if input.options.customer_tier == "enterprise" {
            prepared.instructions = Some("You are a senior support agent. Prioritize SLA commitments.".into());
        }
        Ok(prepared)
    }))
    .stop_when(step_count(12))
    .build();

let result = agent
    .generate(AgentCall::prompt("Where is order #4521?").options(SupportOptions { customer_tier: "enterprise".into() }))
    .await?;
```

## 4. 自定义 Agent

应用可实现 `Agent` trait 组合多个模型或引入外部规划逻辑。核心层不假设 Agent 的内部结构；`Agent` 对象可以直接作为工具的执行函数嵌套使用（子 Agent 模式），此时子 Agent 的 `GenerateTextResult::text()` 通常作为工具输出。

## 5. 待验证

- 【决策】（PV-009，`verification/pv009-override`；2026-09-13 被 [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 2 项取代，见第 6 节）`PreparedCall` 的可覆盖字段使用 `Override<T>::{Keep, Clear, Set(T)}`，`Default` 为 `Keep`，并实现 `From<T>`（→ `Set`）与 `From<Option<T>>`（`None` → `Clear`，即显式传入 `None` 表示清除外层设置）。依据：`..PreparedCall::default()` 结合 `Override::Clear` / `0.2.into()` 的写法在原型中可直读；`Option<Option<T>>` 需要读者记忆两层 `None` 的含义，原型测试 `nested_option_is_ambiguous_to_read` 记录了该问题。

## 6. 实现记录（2026-09-13）

- 【决策】（[ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 1 项）`Agent::Options: Send + 'static`，`call_options::<O>()` 不要求 `DeserializeOwned + JsonSchema`。
- 【决策】（ADR 0013 第 2 项）`PrepareCall<Opt>` trait 的 `prepare_call(&self, PrepareCallInput<Opt> { options, defaults: PreparedCall }) -> BoxFuture<'_, Result<PreparedCall, Error>>`；`defaults` 为 Agent 设置与调用参数合并后的有效值（`instructions`、`model`、`tools`、`tool_choice`、`active_tools`、`tool_order`、`tools_context`、`settings: CallSettings`、`stop_conditions`、`timeout`、`retry_policy`、`include`、`max_tool_concurrency`、`telemetry`），字段置 `None` 即移除。返回 `Future<Output = Result<PreparedCall, Error>> + Send + 'static` 的闭包通过 blanket impl 实现该 trait。
- 【决策】未配置 `stop_when` 时默认 `step_count(20)`；调用级 `timeout` 整体替换 Agent 级 `timeout`（不按字段合并）；调用级 `Hooks` 追加在 Agent 级之后执行（`Hooks::merged`）；`telemetry.function_id` 为空时取 Agent `id`。依据：20 步足以覆盖多数工具循环而不致失控；Agent 级回调先执行使通用日志先于调用特定逻辑；超时整体替换避免两级配置的字段级合并歧义。
- 【事实】`AgentCall<O>` 提供 `new/prompt/messages`（`AgentCall<()>`）、`options<P>()`（切换选项类型）、`cancellation`、`timeout`、`hooks`、`on_step_end`、`on_end`、`streaming() -> AgentStreamCall<O>`；`AgentStreamCall<O>` 增加 `transform`、`include_raw_chunks`、`stream_retries`、`on_error`、`on_chunk`、`on_abort`。请求头追加 `ferrin-agent/tool-loop` 后缀后再由核心层追加 `ferrin/<version>`。
