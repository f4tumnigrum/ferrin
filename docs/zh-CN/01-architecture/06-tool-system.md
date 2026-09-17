# 工具系统

[English](../../01-architecture/06-tool-system.md) | **简体中文**

工具定义位于 `ferrin-tool`，工具调用解析、审批、执行与修复位于 `ferrin-core::generate_text`。

## 1. 工具种类

【决策】工具按种类区分：

| 种类 | 特征 | 执行方 | Schema 来源 |
| --- | --- | --- | --- |
| 函数工具 | 默认种类 | 客户端（有 `execute`）或应用（无 `execute`） | 应用定义 |
| 动态工具 | 输入输出为运行期 JSON | 客户端 | 运行期（MCP 等），输入输出为 `unknown` |
| 供应商定义工具 | 供应商定义、客户端执行 | 客户端 | 供应商 crate 定义（如 Anthropic 计算机操作） |
| 供应商执行工具 | 供应商定义、供应商服务端执行，可选支持延迟结果 | 供应商服务端 | 供应商 crate 定义 |

【决策】工具字段：`description`（字符串或接受 `{context, sandbox}` 的函数）、`input_schema`、`output_schema`、`context_schema`、`execute`、`needs_approval`、`strict`、`input_examples`、`metadata`（传播至工具调用及结果的应用元数据）、`provider_options`（发送给供应商）、`on_input_start`/`on_input_delta`/`on_input_available`、`to_model_output`。

```rust
pub struct Tool {
    kind: ToolKind,
    description: Description,                  // Static(String) | Dynamic(Arc<dyn Fn(&ToolDescriptionContext) -> BoxFuture<'_, String>>)
    input_schema: Schema<JsonValue>,           // typed tools erase to JsonValue here; typing lives in the executor
    output_schema: Option<Schema<JsonValue>>,
    context_schema: Option<Schema<JsonValue>>,
    execute: Option<Arc<dyn ToolExecute>>,
    needs_approval: NeedsApproval,             // Never | Always | Dynamic(fn)
    strict: Option<bool>,
    input_examples: Vec<JsonObject>,
    metadata: Option<JsonObject>,
    provider_options: Option<ProviderOptions>,
    hooks: ToolHooks,                          // on_input_start / delta / available
    to_model_output: Option<Arc<dyn Fn(&JsonValue) -> ToolResultOutput + Send + Sync>>,
}

#[non_exhaustive]
pub enum ToolKind {
    Function,
    Dynamic,
    ProviderDefined { id: String, args: JsonObject },
    ProviderExecuted { id: String, args: JsonObject, supports_deferred_results: bool },
}
```

### 1.1 类型化定义

【决策】工具的类型信息保留在定义时，运行时以 JSON 值流转：

```rust
#[derive(Deserialize, JsonSchema)]
struct GetWeatherInput {
    /// City name, e.g. "Berlin".
    city: String,
}

#[derive(Serialize)]
struct Weather { temperature_c: f32, condition: String }

let get_weather = Tool::function::<GetWeatherInput>()
    .description("Get the current weather for a city.")
    .execute(|input: GetWeatherInput, _ctx: ToolContext| async move {
        Ok(Weather { temperature_c: 21.5, condition: "sunny".into() })
    })
    .build();

let tools = ToolSet::new().insert("get_weather", get_weather)?;
```

【事实】2026-09-13 实现（`crates/ferrin-tool/`）与上述草案的差异：

- `Tool` 的 `description` 为 `Option<Description>`（无描述的工具合法），另有 `title: Option<String>`（参与指纹）与 `caller_definition: Option<ToolCallerDefinition>`（见第 5 节）。`to_model_output` 的签名为 `Fn(ModelOutputArgs { tool_call_id, input, output }) -> ToolResultOutput`。
- 构造入口：`Tool::function::<I>()`（schema 由 `Schema::<I>::derived().erased()` 派生）、`Tool::function_with_schema(Schema<JsonValue>)`、`Tool::dynamic(schema)`、`Tool::provider_defined(id, args)`、`Tool::provider_executed(id, args)`（`.supports_deferred_results(true)`）。`ToolBuilder<I>` 提供 `description`/`description_fn`、`title`、`input_schema`、`output_schema`、`context_schema`、`needs_approval`/`needs_approval_if`、`strict`、`input_example(s)`、`metadata`、`provider_options`、`on_input_start`/`on_input_delta`/`on_input_available`、`to_model_output`、`caller`、`execute`（异步闭包，返回 `Result<O: Serialize, ToolError>`）、`execute_stream`（返回 `Stream<Item = Result<O, ToolError>>`，每项为 `Preliminary`，末项重复为 `Final`）、`execute_with(Arc<dyn ToolExecute>)`、`build`。
- `Tool` 的方法：`definition(name, description) -> spec::ToolDefinition`（函数/动态工具 → `Function`，供应商工具 → `Provider`）、`resolve_description(DescriptionContext)`、`validate_input(name, value)`（错误上下文 `field: "tool input"`）、`validate_context(name, Option<JsonValue>)`（无 `context_schema` 时返回 `None`，有 schema 而未提供上下文时按 `null` 校验，错误上下文 `field: "tool context"`）、`execute(input, ctx) -> Option<ToolOutputStream>`。类型化闭包的输入用 serde 从已校验的 JSON 反序列化，失败为 `ToolError::Message("invalid tool input: ...")`。

`Tool::function::<I>()` 要求 `I: DeserializeOwned + JsonSchema`；`execute` 闭包的返回类型 `O: Serialize` 在内部转换为 `JsonValue`。依据：把工具集的类型信息推导到结果类型在 Rust 中需要每个工具集一个枚举（或宏生成），成本过高；工具级类型 + 结果级 JSON 值 + 类型化提取辅助（`step.tool_result_as::<Weather>("get_weather")`）在可用性与复杂度之间平衡。

【决策】`ferrin-macros` 提供 `#[ferrin::tool]` 属性宏，把带文档注释的 `async fn` 转换为 `Tool` 构造函数；文档注释作为描述，参数结构体作为输入 Schema。宏是可选便利层，生成的代码只调用公共 API。

【事实】2026-09-14 实现（`crates/ferrin-macros/`）：`#[ferrin::tool]` 接受 `[pub] [async] fn name(input: I[, ctx: ToolContext]) -> Result<O, ToolError>`，展开为 `[pub] fn name() -> ::ferrin::tool::Tool`，函数体作为内部函数保留，构造为 `Tool::function::<I>().description(<文档注释>).execute(|input, ctx| name(input[, ctx])).build()`（同步函数以 `core::future::ready` 包装；无文档注释则不设描述；文档注释按行去掉首个空格后以换行拼接并去除首尾空行，参数结构体字段上的 doc 注释经 `schemars` 进入 schema 的 `description`）。语法层检查：引用类型（含 `Vec<&str>` 等嵌套）或显式生命周期 → `tool parameters must be owned types (use String instead of &str)`；类型泛型 → `tool functions cannot be generic`；参数个数不是 1 或 2、`self`、缺少返回类型、`const`/`unsafe`/ABI/可变参数、宏带参数各有对应错误。生成代码引用 `::ferrin::tool::*`（而非 `ferrin_tool`），因此宏只能经门面使用；输入类型的派生需 `#[serde(crate = "ferrin::serde")]`、`#[schemars(crate = "ferrin::schemars")]` 或直接依赖 `serde`/`schemars`。测试位于 `crates/ferrin/tests/suite/tool_macro.rs` 与 `crates/ferrin/tests/ui/`（1 个编译通过用例、7 个编译失败用例）。

### 1.2 工具集

【决策】`ToolSet` 以工具名为键索引工具，工具名在集合内唯一。

`ToolSet` 是 `IndexMap<ToolName, Arc<Tool>>`（保持插入顺序），提供 `insert`、`get`、`names`、`filter(active)`、`merge`。工具名重复插入返回错误而非覆盖。

【事实】2026-09-13 实现：`insert(self, name, tool) -> Result<Self, DuplicateToolError>`（链式构造需 `?`）、`try_insert(&mut self, ..)`、`try_insert_arc`、`replace`（保留位置）、`remove`（保持其余顺序）、`get`、`contains`、`names`、`iter`、`len`/`is_empty`、`filter_active(&[ToolName])`（保持本集合顺序，忽略未知名）、`merge(self, other) -> Result<Self, DuplicateToolError>`、`ordered(&[ToolName])`（列出的在前按给定顺序，其余按名称字母序，即第 6 节的 `tool_order` 规则）；`ToolSet: Clone`（共享 `Arc<Tool>`）。

## 2. 执行契约

【决策】执行函数可以返回单个值，也可以返回一个流：流的中间值作为 `preliminary: true` 的结果，最后一个值为最终结果。执行上下文 `ToolContext` 包含 `tool_call_id`、`messages`（当前步骤发送给模型的消息）、取消令牌、`tools_context`（工具上下文）与 `sandbox`。依据：长时间运行的工具（搜索、代码执行）需要向应用报告进度，初步结果让流式界面在工具完成前就能展示中间状态。

```rust
pub trait ToolExecute: Send + Sync {
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> BoxStream<'static, Result<ToolOutput, ToolError>>;
}

pub enum ToolOutput {
    Preliminary(JsonValue),
    Final(JsonValue),
}

pub struct ToolContext {
    pub tool_call_id: ToolCallId,
    pub messages: Arc<[Message]>,
    pub cancellation: CancellationToken,
    pub tools_context: Option<JsonValue>,      // validated against context_schema
    pub sandbox: Option<Arc<dyn Sandbox>>,     // feature "sandbox"
}
```

单值执行函数通过 `From` 适配为只产出一个 `Final` 的流。依据：统一为流可以让核心层用同一路径处理初步结果与最终结果。

【事实】2026-09-13 实现：`ToolExecute::execute(&self, JsonValue, ToolContext) -> ToolOutputStream`（`BoxStream<'static, Result<ToolOutput, ToolError>>`）；`ToolContext` 字段如上，`sandbox` 字段与 `with_sandbox` 仅在 feature `sandbox` 下存在，`ToolContext::new(tool_call_id)` 后以 `with_messages`/`with_cancellation`/`with_tools_context` 填充。`execute_to_completion(stream, on_preliminary)` 驱动流并返回最终值，流在没有 `Final` 的情况下结束时返回 `ToolError::Message`。`ToolError` 定义于本 crate（见[错误模型](12-error-model.md)第 3 节），提供 `message`、`json`、`from_error`、`with_cause`、`is_cancelled` 与 `From<serde_json::Error>`。`model_output::create_tool_model_output(tool, tool_call_id, input, output, ErrorMode::{None, Text, Json})` 与 `tool_error_output(&ToolError)` 实现工具输出规范化规则（`Json` 错误 → `error-json`，其余 → `error-text`；`error_message` 对 `null` 返回 `unknown error`、对带 `message` 字段的对象取该字段）。

【决策】工具错误被捕获并作为 `tool-error` 内容（非致命）纳入步骤结果，随后作为 `error-text`/`error-json` 发回模型；只有被取消时按取消处理。依据：模型通常能够根据错误文本调整参数重试，直接终止调用会丢失这一恢复机会。

## 3. 工具调用解析与修复

【决策】将生效工具定义的 metadata 复制到 `ParsedToolCall::tool_metadata`，包括输入无效的已知工具调用。修复成功后使用修复目标工具的元数据；没有工具定义的动态供应商调用不包含工具元数据。结果、执行错误、流式中间结果和审批重放的拒绝输出携带同一可选 JSON 对象，并与供应商响应元数据分开。审批重放从当前工具定义恢复元数据，因为消息历史不携带可信工具定义。新增序列化字段在历史记录中默认缺省（2026-09-17；参见 core 工具元数据测试）。

【决策】工具调用解析规则：

1. 工具名不在工具集中：构造 `NoSuchTool` 错误；若配置了 `repair_tool_call`，以该错误调用修复函数。
2. 输入字符串为空时按 `{}` 处理；解析 JSON 并按 `input_schema` 校验，失败构造 `InvalidToolInput` 错误并尝试修复。
3. 修复函数返回 `None` 时放弃；返回新调用时重新解析；修复函数出错时包装为修复错误。
4. 最终无法解析的调用不抛出，而是作为 `invalid: true, dynamic: true` 的工具调用进入内容，并附 `error` 字段；随后由 `tool-error` 内容把错误反馈给模型。
5. 若 `tool_choice` 指定了具体工具而模型调用了其他工具，视为工具选择违规（作为无效调用处理）。
6. 可选的按工具名配置的 `refine_tool_input` 在校验通过后对输入做同形改写，用于执行、事件与遥测。
7. 供应商执行工具的调用不做本地 Schema 校验，输入原样解析为 JSON。

```rust
pub trait ToolCallRepair: Send + Sync {
    fn repair(
        &self,
        request: RepairRequest<'_>,     // { tool_call, tools, input_schema, messages, system, error: NoSuchTool | InvalidToolInput }
    ) -> BoxFuture<'_, Result<Option<spec::ToolCall>, Box<dyn Error + Send + Sync>>>;
}
```

## 4. 审批

### 4.1 判定

【决策】审批状态解析：

- 可选的调用级审批策略（`ApprovalPolicy`）优先级最高；其次是每工具的用户配置；最后是工具自身的 `needs_approval`（布尔或函数）。
- 结果为四态：`not-applicable`（无需审批，直接执行）、`approved`、`denied {reason?}`、`user-approval {reason?}`（需要外部审批）。
- 已在历史消息中收到审批响应的调用，按响应处理。

```rust
pub enum ApprovalStatus {
    NotApplicable,
    Approved,
    Denied { reason: Option<String> },
    UserApproval { reason: Option<String> },
}

pub trait ApprovalPolicy: Send + Sync {
    fn resolve(&self, call: &ParsedToolCall, ctx: &ApprovalContext) -> BoxFuture<'_, ApprovalDecision>;
}
```

（2026-09-13：实现签名为 `resolve<'a>(&'a self, call: &'a ParsedToolCall, ctx: ApprovalContext<'a>) -> BoxFuture<'a, Option<ApprovalStatus>>`，见第 11 节。）

【决策】由外部或内嵌引擎判定的策略（OPA REST Data API、经 `regorus` 的 Rego）通过同一 trait 接入：`ferrin_policy::policy_approval(client, path)` 实现 `ApprovalPolicy`，把 `allow` / `deny` / `requires-approval` 映射到上述状态、`not-applicable` 映射为 `None`，判定失败时默认拒绝。见[策略化工具审批](18-policy-approval.md)与 [ADR 0020](../04-decisions/2026-09-15-0020-policy-based-tool-approval.md)。

### 4.2 审批请求与响应

【决策】需要用户审批的调用不执行，而是产生 `tool-approval-request {approval_id, tool_call_id, tool_name, input, reason?, signature?}` 内容并终止循环（继续条件不满足）。应用把审批响应 `tool-approval-response {approval_id, approved, reason?, provider_executed?}` 追加到最后一条工具消息后再次调用（签名随请求保存在助手消息中）；核心层：

1. 收集最后一条工具消息中的审批响应，在历史助手消息中找到对应的审批请求与工具调用，找不到时报 `ToolCallNotFoundForApproval` 错误。
2. 若配置了审批密钥，校验签名；不匹配抛 `InvalidToolApprovalError`。
3. 重新校验工具输入、重新解析审批策略（防止历史消息被篡改）。
4. 通过的调用执行；被拒绝的产生 `execution-denied` 工具结果。

【决策】签名算法：HMAC-SHA256，密钥为应用提供的 `tool_approval_secret`，载荷为 JSON 数组 `["ferrin-tool-approval-v1", approval_id, tool_call_id, tool_name, input_digest]`，其中 `input_digest` 为规范化输入 JSON 的 SHA-256；签名为 base64url。依据：域分隔字符串防止签名被挪作他用；对输入摘要而非原文签名使载荷长度固定，规范化保证键序无关。

【决策】Ferrin 采用同一结构但更换域分隔字符串为 `"ferrin-tool-approval-v1"`，输入规范化采用键排序的紧凑 JSON。签名密钥类型为 `secrecy::SecretBox<[u8]>`。依据：审批重放来自应用持久化的消息历史，签名是防止篡改的必要机制；域分隔字符串区分产品避免跨系统重放。

### 4.3 工具指纹与漂移

【决策】`fingerprint_tools` 对工具的名称、描述、Schema 生成摘要，`detect_tool_drift` 比较两个指纹集合报告新增、删除、变更，用于在审批流程跨请求时检测工具定义是否变化。依据：审批响应引用的是发起请求时的工具定义，定义在两次请求之间变化时审批不应继续有效。

Ferrin 在 `ferrin_tool::fingerprint` 提供等价函数，返回 `ToolDrift { added, removed, changed }`。

【事实】2026-09-13 实现：`fingerprint_tools(&ToolSet) -> BTreeMap<ToolName, String>` 对 `{ description: {type: "string", value} | {type: "function"} | {type: "none"}, inputSchema, title? }` 做键排序紧凑 JSON 的 SHA-256（base64url 无填充，`canonical_json`/`hash_canonical` 公开）；`title` 缺失时省略该键。指纹基线由 Ferrin 自身生成，不与其他实现比较，因此规范化 JSON 的细节（数字格式、缺失键的处理）只需在 Ferrin 内部保持稳定。`detect_tool_drift(current, baseline)` 按名称字母序输出三类差异。

## 5. 调用方限制

【决策】`tool_callers` 配置每个工具可由哪些调用方触发：本地绑定的“调用工具”或供应商；不允许的调用被视为无效。依据：限制调用方可以防止模型直接调用只应由其他工具间接触发的高权限工具。

【决策】Ferrin 纳入 `ToolCallers` 配置，形态为 `HashMap<ToolName, Vec<ToolCaller>>`，`ToolCaller::{Provider, Tool(ToolName)}`。

【决策】2026-09-13 实现把直接调用的变体命名为 `ToolCaller::Direct`，以避免与 `ToolCallerDefinition::Provider`（调用方工具由供应商代为发起调用）混淆。`ToolCallerDefinition::{Local(bind: Fn(ToolSet) -> Tool), Provider(prepare: Fn(Option<ProviderOptions>) -> ProviderOptions)}` 通过 `ToolBuilder::caller` 附着在调用方工具上；`callers::validate_tool_callers(&ToolSet, &ToolCallers)` 对未知工具或无调用方定义的调用方返回 `InvalidArgumentError { argument: "tool_callers" }`；`callers::prepare_tools_for_callers` 产出 `PreparedToolCallers { execution_tools, model_tools }`：本地调用方的被调工具绑定进调用方并从模型工具集移除，供应商调用方的被调工具改写 `provider_options`，无 `Direct`/供应商调用方的工具不发送给模型。

## 6. 活动工具与顺序

【决策】`active_tools` 限制本步骤发送给模型的工具子集而不改变结果类型；`tool_order` 控制发送顺序。两者均可在 `prepare_step` 中按步骤覆盖。依据：按步骤裁剪工具集可以引导模型的工具选择并减少提示长度，稳定的发送顺序有利于供应商侧提示缓存命中。

## 7. 工具上下文

【决策】`tools_context` 参数为工具执行提供共享上下文，按工具声明的 `context_schema` 校验。依据：请求级的用户身份、租户等信息需要传给工具而不应出现在提示中。

【决策】Ferrin 的 `tools_context: Option<JsonValue>` 在调用时按各工具的 `context_schema` 校验一次；未定义 `context_schema` 的工具收到 `None`。

## 8. 沙箱

【决策】`SandboxSession` 提供 `description`、`read_file`（字节流）、`read_binary_file`、`read_text_file`（编码、行范围）、`write_file`/`write_binary_file`/`write_text_file`、`spawn`（返回 `SandboxProcess {stdout, stderr, wait, kill}`）、`run`（等待完成并收集输出）；命令选项含 `command`、`working_directory`、`env` 与取消令牌。依据：这一接口覆盖工具在隔离环境中读写文件与运行命令的最小集合，具体沙箱（容器、远程）由应用实现。

```rust
pub trait Sandbox: Send + Sync {
    fn description(&self) -> &str;
    fn read_file(&self, opts: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<BoxStream<'static, io::Result<Bytes>>>>>;
    fn read_binary_file(&self, opts: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<Bytes>>>;
    fn read_text_file(&self, opts: ReadTextFileOptions) -> BoxFuture<'_, io::Result<Option<String>>>;
    fn write_file(&self, opts: WriteFileOptions) -> BoxFuture<'_, io::Result<()>>;
    fn spawn(&self, opts: ProcessOptions) -> BoxFuture<'_, io::Result<Box<dyn SandboxProcess>>>;
    fn run(&self, opts: ProcessOptions) -> BoxFuture<'_, io::Result<ProcessResult>>;
}
```

Ferrin 只定义 trait 与一个本地进程实现 `LocalProcessSandbox`（仅用于测试与示例，文档中明确标注不提供隔离）。

【决策】2026-09-13 `Sandbox`、`SandboxProcess` 与 `LocalProcessSandbox` 位于 `ferrin_tool::sandbox`，由 feature `sandbox` 启用（引入 `bytes` 与 tokio 的 `process`/`fs`/`io-util`）。依据：沙箱是工具执行上下文的一部分，放在工具 crate 便于 `ToolContext::sandbox` 与 `DescriptionContext::sandbox` 直接引用；`ferrin-testing` 依赖核心层，不适合承载被核心层引用的 trait。

【事实】2026-09-13 实现的 trait 形态：`read_file -> Option<ByteStream>`（`BoxStream<'static, io::Result<Bytes>>`）、`read_binary_file -> Option<Bytes>`、`read_text_file(ReadTextFileOptions { path, encoding, start_line, end_line, cancellation }) -> Option<String>`、`write_file(WriteFileOptions<ByteStream>)`、`write_binary_file(WriteFileOptions<Bytes>)`、`write_text_file(WriteFileOptions<String>)`、`spawn -> Box<dyn SandboxProcess>`、`run -> ProcessResult { exit_code, stdout, stderr }`；`SandboxProcess` 提供 `pid`、`take_stdout`/`take_stderr`（各取一次）、`wait -> i32`、`kill`（幂等）。取消令牌（`cancellation` 字段）置于各选项结构中，触发后返回 `io::ErrorKind::Interrupted` 并终止进程。`LocalProcessSandbox::new(root)` 以 `/bin/sh -c`（Windows 为 `cmd /C`）执行命令，路径相对 `root` 解析但不阻止绝对路径与 `..`；文本编码仅支持 UTF-8（其他值返回 `Unsupported`），行范围为 1 起始的闭区间，越界按文件末尾截断。

## 9. 工具执行的超时与遥测

- 工具超时：`Timeout::tool` 为默认值，`Timeout::per_tool[name]` 覆盖；超时视为工具错误（非致命）。
- 每次执行触发 `Telemetry::on_tool_execution_start/end`，并在 `tracing` 中开启 `ferrin.tool` span（字段：`tool.name`、`tool.call_id`、耗时）。
- 步骤结果中每个工具结果附 `tool_execution_ms`。

## 10. 待验证

- 【事实】（PV-004，`verification/pv004-schema`）`schemars` 1.2.2 `SchemaSettings::draft07()` 的输出：`Option<原始类型>` → `"type": [T, "null"]`；`Option<引用类型>` → `anyOf: [{$ref}, {type: null}]`；`Option` 字段不出现在 `required` 中；单元变体枚举 → `type: string` + `enum`；内部标签枚举 → `oneOf`，每个分支含 `required` 的标签字段与 `const`；定义位于 `definitions`；不生成 `additionalProperties`。OpenAI 严格模式要求对象声明 `additionalProperties: false`、全部属性列入 `required`，且不接受 `propertyNames`；可选字段需要以可空类型表达。
- 【决策】`ferrin-schema` 提供 `SchemaTransform::openai_strict()`：递归设置 `additionalProperties: false`、移除 `propertyNames`、把全部属性加入 `required`，并把原本可选的属性改为可空（把完整属性 Schema 包一层 `anyOf: [原始 Schema, {type: null}]`，保留非空值的 `enum`、`const` 和组合约束）。指向 Schema 内已声明资源的 JSON Pointer 引用随节点移动而调整，涵盖仅片段、相对和绝对 URI；保留嵌套 `$id` 作用域和 URI 编码名称，不获取外部资源。PV-004 原型记录上述派生 Schema 形状；`crates/ferrin-schema/tests/suite/transform.rs` 中的回归测试验证完整 Schema 包装对可选 enum、const 和组合约束的空值语义（2026-09-15）。默认工具 Schema 只做 `additionalProperties: false`，严格变换仅在 OpenAI 适配器 `strict: true` 时应用。
- 【事实】（PV-005，`verification/pv005-static-capture`）在 `Tool::function` 的约束（`F: Fn(I) -> Fut + Send + Sync + 'static, Fut: Future + Send + 'static`）下，闭包捕获局部引用时 rustc 报 `E0597: borrowed value does not live long enough ... argument requires that ... is borrowed for 'static`，并附注指向 `'static` 约束；`async fn` 带引用参数时报 `lifetime may not live long enough ... closure implements Fn, so references to captured variables can't escape the closure`。两条诊断都定位到用户代码。
- 【决策】`#[ferrin::tool]` 宏展开为对 `Tool::function` 的调用（复用上述诊断），并在语法层检查被标注函数的参数类型：出现引用类型（`&T`、`&str`、`&[T]`）或显式生命周期时直接 `compile_error!("tool parameters must be owned types (use String instead of &str)")`，把最常见的错误前移到宏输入处。`trybuild` 用例位于 `crates/ferrin/tests/ui/`（见测试规范）。

## 11. 实现记录（2026-09-13）

- 【决策】`ApprovalPolicy::resolve` 返回 `Option<ApprovalStatus>`：`None` 表示策略不表态，判定回退到工具自身的 `needs_approval`；`ApprovalStatus` 自身实现 `ApprovalPolicy`（常量策略），同步闭包 `Fn(&ParsedToolCall, &ApprovalContext<'_>) -> Option<ApprovalStatus>` 经 `ApprovalPolicyFn` 包装后可作为策略。依据：调用级策略不表态时回退到工具级配置，`Option` 直接表达该优先级链，不需要单独的 `ApprovalDecision` 类型。
- 【决策】`PrepareStep` 为同步闭包 `Fn(&PrepareStepContext<'_>) -> StepOverrides` 提供 blanket impl，需要异步逻辑的应用实现 trait 本身。依据：绝大多数 `prepare_step` 用法只是按步骤号切换模型或工具集，同步闭包避免 `Box::pin(async move { .. })` 样板。
- 【事实】`RefineToolInputs` 按工具名保存 `Arc<dyn Fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, Error>>>`，在输入通过 schema 校验之后、执行之前改写输入；改写结果写回 `ParsedToolCall.input` 并进入响应消息。
- 【事实】`ToolApprovalRequestContent { approval_id, tool_call: ParsedToolCall, reason, is_automatic }` 携带完整的已解析工具调用而非仅 `tool_call_id`，`StepContent::ToolApprovalResponse(ToolApprovalResponseContent { approval_id, tool_call, approved, reason, provider_executed })` 记录重放时的判定结果。依据：审批 UI 需要展示工具名与输入，避免应用在历史消息中二次查找。
- 【事实】`DescriptionContext::with_tool_context(JsonValue)` 供生成循环之外（批处理、实时会话）解析动态描述。

【决策】 解析与修复使用本步骤发给提供商的同一工具集，先应用调用者限制，再应用活动工具筛选。仅由本地调用者访问的工具仍可供该调用者使用，但模型直接返回其名称时不能选中它或触发其输入回调。

【决策】 审批恢复在单次调用内按审批 ID 与工具调用 ID 对决定去重。审批结论或提供商执行标记冲突时，在执行任何工具之前失败；重复的相同决定只调度一次执行。这不提供跨调用的恰好一次执行保证。

【决策】 审批恢复在启动任何工具执行之前，校验所有已批准且可执行的客户端工具的当前上下文。上下文构造将校验失败作为 `tools_context` 的 `InvalidArgument` 错误向上传递，不能将失败替换为缺失上下文。

【决策】严格 Schema 转换对任意键字典返回错误，不会将其关闭或返回不受支持的 Schema；见 [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md)。`apply`、`applied`、`to_openai_strict` 和 `Schema::transformed` 返回 `Result`；原地转换失败时输入不变。

【决策】本地 sandbox 在创建进程前检查取消，并在文件和进程输出流的生命周期内持续响应取消。每个创建的进程由 `JoinSet` 持有的监督任务管理，即使应用尚未调用 `wait`，取消也会终止并回收进程；丢弃进程对象会中止监督任务并终止其持有的子进程。取消的读取返回一次 `Interrupted` 错误后结束。

【事实】2026-09-15：`ferrin-policy` 在该契约之上实现 `policy_approval`、`shadow`、`with_default` 与 `capability_middleware`，核心层无需改动；其覆盖清单见[策略化工具审批](18-policy-approval.md)第 7 节。

【决策】`Tool::into_builder()` 将工具重新开放为 `ToolBuilder<JsonValue>`，完整保留定义、schema、参数、元数据、执行器、调用者绑定及钩子；调用方可附加或替换执行行为，无需重建供应商工厂。

【决策】供应商路由元数据随所有审批结果传播至响应消息的供应商参数，包括自动拒绝、重放拒绝、重放成功及重放错误。`ToolOutputDenied::provider_metadata` 保存原始调用的供应商参数，独立于工具定义元数据；旧序列化拒绝结果缺失此字段时默认为无。拒绝也必须保留并行工具包装器标识，供应商才能接收完整的分组结果（2026-09-17；ADR 0021）。
