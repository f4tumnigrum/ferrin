# API 设计原则

[English](../../02-api/01-api-design-principles.md) | **简体中文**

本文档约束 `ferrin` 门面 crate 与 `ferrin-core` 的公共 API 形态。

## 1. 调用形态

【决策】入口函数返回构建器，构建器实现 `IntoFuture`：

```rust
pub fn generate_text(model: impl Into<LanguageModelRef>) -> GenerateText<()>;
pub fn stream_text(model: impl Into<LanguageModelRef>) -> StreamText<()>;
pub fn embed(model: impl Into<EmbeddingModelRef>, value: impl Into<String>) -> Embed;
// ...

impl<O: Send + 'static> IntoFuture for GenerateText<O> {
    type Output = Result<GenerateTextResult<O>, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;
}
```

依据：

- 一次调用有数十个可选参数（采样参数、工具、停止条件、回调、遥测）；位置参数或单一配置结构体都会让调用点难读且难以向后兼容地扩展，Rust 中的惯用等价物是构建器。
- `IntoFuture` 让 `.await` 直接执行，无需 `.send()`/`.run()` 一类的终结方法；`reqwest::RequestBuilder` 采用相同模式。
- 构建器方法新增不破坏调用方，符合保守 API 演进。

构建器全部 `Send + 'static`，可在构造后跨任务传递。

## 2. 参数类型

| 场景 | 形态 | 示例 |
| --- | --- | --- |
| 模式选择 | 枚举 | `ToolChoice::Required`、`Chunking::Line` |
| 开关 | 命名方法，无布尔参数 | `.allow_system_in_messages()` 而非 `.system_in_messages(true)` |
| 可选值 | 方法调用即设置，不调用即默认 | `.temperature(0.2)` |
| 集合 | `impl IntoIterator<Item = impl Into<T>>` | `.stop_sequences(["END"])` |
| 回调 | `impl Fn(...) -> Fut + Send + Sync + 'static` | `.on_step_end(|step| async move { ... })` |
| 模型 | `impl Into<LanguageModelRef>`，接受 `&Arc<dyn DynLanguageModel>`、`Arc<...>`、具体模型类型、`&str`（需默认注册表） | `generate_text(&model)` |
| 时长 | `std::time::Duration` | `.timeout(Duration::from_secs(30))` |
| 二进制 | `bytes::Bytes` 或 `impl Into<Bytes>` | `UserPart::image_bytes(data)` |

【决策】不接受布尔或裸 `Option` 位置参数。依据：`foo(false)`、`bar(None)` 在调用点不表达含义，读者必须查看签名；枚举与命名方法在调用点即自说明，并且可以在不破坏调用方的前提下增加变体。

## 3. 结果类型

- 结果结构体字段公开（`pub`），派生 `Debug`、`Clone`；便捷访问器（`text()`、`tool_calls()`）以方法提供。
- 结构化输出通过泛型参数 `O` 表达，默认 `()`。
- 错误统一为 `ferrin::Error`；不在公共签名中暴露 `Box<dyn Error>` 以外的第三方错误类型。

## 4. 类型稳定性

- 公共枚举与错误标记 `#[non_exhaustive]`。
- 公共结构体若预期增加字段，标记 `#[non_exhaustive]` 并提供构造函数或构建器。
- 第三方类型出现在公共 API 中的白名单：`serde_json::Value/Map`、`bytes::Bytes`、`url::Url`、`http::{HeaderMap, StatusCode, Method}`、`chrono::DateTime<Utc>`、`tokio_util::sync::CancellationToken`、`futures_core::Stream`、`schemars::JsonSchema`（trait bound）、`secrecy::SecretString`。这些 crate 的主版本升级视为 Ferrin 的破坏性变更。

## 5. 命名

- 函数与方法：snake_case 动词短语（`generate_text`、`wrap_language_model`）。
- 类型：使用领域内通行的英文名词（`StepResult`、`StopCondition`、`ToolSet`），不带版本号或稳定性前缀后缀。
- 不使用缩写，`Id` 结尾表示标识符类型。
- 供应商 crate 的类型以供应商名前缀（`OpenAiProvider`、`AnthropicSettings`），大小写遵循 Rust 驼峰（`OpenAi` 而非 `OpenAI`）。

## 6. 文档要求

- 每个公共项有文档注释，说明用途、默认值、错误条件；示例代码可编译（doctest）。
- 每个入口函数的文档包含一个最小示例与一个带工具的示例。
- 仍在演进中的能力（供应商 API 本身处于 beta/preview，或 Ferrin 尚未收敛接口）在文档注释中标注 `# Stability` 段落说明其在 `0.y` 阶段可能变更；不使用 `experimental_` 前缀。

## 7. 门面 crate 结构

```rust
// ferrin/src/lib.rs
pub use ferrin_core::*;
pub mod spec { pub use ferrin_spec::*; }
pub mod schema { pub use ferrin_schema::*; }
pub mod prelude {
    pub use ferrin_core::{generate_text, stream_text, embed, embed_many, step_count, has_tool_call, Output, ToolSet, Tool, Message, Error};
    pub use ferrin_spec::{Warning, Usage, FinishReason};
    pub use futures_util::StreamExt as _;
}
#[cfg(feature = "openai")] pub mod openai { pub use ferrin_openai::*; }
#[cfg(feature = "anthropic")] pub mod anthropic { pub use ferrin_anthropic::*; }
#[cfg(feature = "mcp")] pub mod mcp { pub use ferrin_mcp::*; }
#[cfg(feature = "otel")] pub mod otel { pub use ferrin_otel::*; }
#[cfg(feature = "macros")] pub use ferrin_macros::tool;
```

## 8. 关键取舍清单

| 常见做法 | Ferrin | 原因 |
| --- | --- | --- |
| 单一配置对象参数 | 构建器 | Rust 惯例；可扩展 |
| 字符串模型 ID 经全局默认供应商解析 | 需显式默认注册表 | 无隐式网络访问 |
| 流式入口同步返回，配置错误进入流 | `stream_text(...).await?` 建立首步请求后返回 | 调用点用 `?` 处理配置错误 |
| 结果对象 tee 出多个消费视图 | 单事件流 + `Completion` | 背压与所有权 |
| `experimental_*` 前缀与弃用别名链 | 不设 | 新产品无历史包袱 |
| 输出格式为运行时可选字段 | 泛型 `O` | 编译期保证 |
| 由工具集泛型推导类型化结果 | 定义时类型化、结果为 JSON + 提取辅助 | Rust 泛型成本 |
| 独立的对象生成入口 | 仅 `generate_text(...).output(...)` | 单一路径 |
