# 错误模型

## 1. 分层

| 层 | 类型 | 说明 |
| --- | --- | --- |
| 规范层 | `ferrin_spec::error::ProviderError` 及各具体错误结构 | 适配器可返回的错误集合，见 [Provider 规范层](04-provider-spec.md) 第 6 节。 |
| Schema 层 | `ferrin_schema::{TypeValidationError, JsonParseError, SchemaError}` | 校验与解析错误。 |
| 工具层 | `ferrin_tool::ToolError` | 工具执行返回的错误（非致命，会反馈给模型）。 |
| 核心层 | `ferrin_core::Error` | 应用可见的统一错误枚举。 |
| MCP | `ferrin_mcp::McpError` | JSON-RPC、传输、协议协商错误；进入核心层时包装为 `Error::Mcp`。 |

## 2. 核心层错误

【事实】核心层需要区分的失败类别：下载失败、数据内容无效、消息角色无效、流部件无效、审批无效、工具输入无效、MCP 客户端错误、消息转换失败、无图像/对象/输出/语音/转写生成、未指定输出、供应商或工具不存在、重试耗尽、审批对应的工具调用不存在、工具调用修复失败、工具选择违规、超时与取消。

【决策】核心层使用单一 `#[non_exhaustive]` 枚举 `Error`，每个变体携带结构化字段，`source()` 链保留底层原因；不需要运行时类型标记，因为 Rust 类型在同一 crate 版本下即唯一。

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error("retries exhausted after {attempts} attempts: {reason:?}")]
    Retry { reason: RetryReason, attempts: u32, errors: Vec<ProviderError> },

    #[error("timeout ({scope:?}) after {elapsed:?}")]
    Timeout { scope: TimeoutScope, elapsed: Duration },

    #[error("operation cancelled")]
    Cancelled,

    #[error("invalid argument `{argument}`: {message}")]
    InvalidArgument { argument: &'static str, message: String },

    #[error("invalid prompt: {message}")]
    InvalidPrompt { message: String },

    #[error("message conversion failed: {message}")]
    MessageConversion { message: String, original_message: Box<Message> },

    #[error("download failed for {url}")]
    Download { url: Url, status_code: Option<StatusCode>, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("invalid data content")]
    InvalidDataContent { #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("no such tool `{tool_name}`")]
    NoSuchTool { tool_name: ToolName, available_tools: Vec<ToolName> },

    #[error("invalid input for tool `{tool_name}`")]
    InvalidToolInput { tool_name: ToolName, tool_input: String, #[source] cause: Box<dyn std::error::Error + Send + Sync> },

    #[error("tool call repair failed")]
    ToolCallRepair { original: Box<Error>, #[source] cause: Box<dyn std::error::Error + Send + Sync> },

    #[error("tool choice violated: expected `{expected}`, got `{actual}`")]
    ToolChoiceViolation { expected: ToolName, actual: ToolName },

    #[error("tool call `{tool_call_id}` not found for approval `{approval_id}`")]
    ToolCallNotFoundForApproval { tool_call_id: ToolCallId, approval_id: ApprovalId },

    #[error("invalid tool approval: {message}")]
    InvalidToolApproval { approval_id: ApprovalId, message: String },

    #[error("no structured output generated")]
    NoObjectGenerated { text: Option<String>, response: ResponseMetadata, usage: Usage, finish_reason: FinishReason, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("no output generated")]
    NoOutputGenerated,

    #[error("no image generated")]
    NoImageGenerated { responses: Vec<ResponseMetadata> },
    #[error("no speech generated")]
    NoSpeechGenerated { responses: Vec<ResponseMetadata> },
    #[error("no transcript generated")]
    NoTranscriptGenerated { responses: Vec<ResponseMetadata> },
    #[error("no video generated")]
    NoVideoGenerated { responses: Vec<ResponseMetadata> },          // 2026-09-13, see §6

    #[error("no such provider `{provider_id}`")]
    NoSuchProvider { provider_id: ProviderId, available_providers: Vec<ProviderId>, model_id: String, model_kind: ModelKind },

    #[error("no default registry configured for model id `{model_id}`")]
    NoDefaultRegistry { model_id: String },

    #[error("invalid stream part: {message}")]
    InvalidStreamPart { message: String },

    #[error(transparent)]
    Mcp(#[from] McpError),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
```

### 2.1 分类方法

```rust
impl Error {
    pub fn is_retryable(&self) -> bool;     // Provider(ApiCall{is_retryable}) or Retry with retryable last error
    pub fn is_cancelled(&self) -> bool;     // Cancelled, or Timeout caused by caller cancellation
    pub fn status_code(&self) -> Option<StatusCode>;
    pub fn kind(&self) -> ErrorKind;        // stable, serializable coarse category
}
```

`ErrorKind` 是可序列化的粗粒度类别（`Provider`、`Retry`、`Timeout`、`Cancelled`、`InvalidInput`、`Tool`, `Output`、`NotFound`、`Mcp`、`Other`），用于遥测与日志的低基数标签。

【决策】2026-09-13 规范层新增 `ProviderError::Cancelled`（见 [Provider 规范层](04-provider-spec.md)第 6 节）。核心层在 `From<ProviderError> for Error` 中把它归并为 `Error::Cancelled`，因此 `is_cancelled()` 与 `kind() == ErrorKind::Cancelled` 对来自适配器的取消同样成立；重试策略在看到该变体时立即停止。

### 2.2 与流事件的关系

流中的错误以 `StreamEvent::Error { error: StreamErrorInfo }` 传递，`StreamErrorInfo` 由 `Error` 投影而来（`kind`、`message`、`status_code`、`is_retryable`、`provider_data`）。`Completion` 在流因错误终止时返回 `Err(Error)`。

【事实】流式调用中错误可能在部分输出已交付后才发生，此时无法再以 `Result` 返回值报告。

## 3. 工具错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{message}")]
    Message { message: String, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },
    #[error("tool returned error payload")]
    Json { value: JsonValue },
    #[error("tool execution timed out after {0:?}")]
    Timeout(Duration),
    #[error("tool execution cancelled")]
    Cancelled,
}
```

【决策】工具错误不终止生成循环，而是生成 `tool-error` 内容并以 `error-text`/`error-json` 反馈模型。依据：模型通常能够根据错误文本调整参数重试。

`ToolError::Message` 转为 `error-text`，`ToolError::Json` 转为 `error-json`；`Timeout` 转为 `error-text`（含超时时长）；`Cancelled` 终止整个调用（映射为 `Error::Cancelled`）。

## 4. 错误消息约定

- 消息以小写字母开头、不含句号结尾，与 Rust 标准库风格一致。
- 消息不包含密钥、请求头或完整请求体；`ApiCallError::response_body` 保留原文，但 `Display` 输出截断到 2 KiB（2 KiB 足以容纳供应商错误体中的诊断信息，同时避免把整页 HTML 错误写入日志）。
- `Debug` 输出对 `Headers` 中的 `authorization`、`x-api-key`、`cookie` 等敏感头做遮蔽。

## 5. 待验证

- 【事实】（PV-013，`verification/pv013-error-size`）按本章变体原样内联时 `Error` 为 360 字节；对 `Provider`（`ProviderError` 240 字节）、`Download`（`Url` 88 字节）、`InvalidToolInput`、`NoObjectGenerated`（`ResponseMetadata` 232 字节）、`NoSuchProvider`、`Mcp`（104 字节）的载荷装箱后为 56 字节，`static_assertions::const_assert!(size_of::<Error>() <= 128)` 通过。
- 【决策】上述六个变体的载荷以 `Box<...Details>` 或 `Box<ProviderError>` 承载；`clippy.toml` 的 `large-error-threshold` 保持 128；`ferrin-core` 的 `error_tests.rs` 保留该 `const_assert!`。

## 6. 实现记录（2026-09-13）

- 【事实】按 PV-013 的结论，`Error::Provider(Box<ProviderError>)`、`Download(Box<DownloadDetails>)`、`InvalidToolInput(Box<InvalidToolInputDetails>)`、`NoObjectGenerated(Box<NoObjectGeneratedDetails>)`、`NoSuchProvider(Box<NoSuchProviderDetails>)` 以装箱载荷实现；`InvalidArgument.argument` 为 `String`；`InvalidDataContent` 增加 `message`；`Mcp(BoxError)` 为类型擦除的载荷（核心层不依赖 `ferrin-mcp`）。`From<ProviderError>` 把 `ProviderError::Cancelled` 映射为 `Error::Cancelled`。`static_assertions::const_assert!(size_of::<Error>() <= 128)` 由测试保证。
- 【决策】增加 `Error::Stream(Box<StreamError>)`：供应商流的 `Error` 分片在非流式消费路径（`stream_transcribe` 的 `text_stream()`、批处理结果流）转换为该变体；`Error::ToolChoiceNotSatisfied { expected: Option<ToolName> }`（见[生成循环与流式](07-generation-loop-and-streaming.md)第 6 节）；`Error::NoVideoGenerated { responses }` 对应 `generate_video` 在所有调用都未返回视频时的失败。
- 【事实】`ErrorKind` 映射：`Provider`、`Stream` → `Provider`；`Retry` → `Retry`；`Timeout`；`Cancelled`；`InvalidArgument`、`InvalidPrompt`、`MessageConversion`、`Download`、`InvalidDataContent`、`InvalidStreamPart` → `InvalidInput`；工具相关（含 `ToolChoiceNotSatisfied`）→ `Tool`；`NoObjectGenerated`、`NoOutputGenerated`、`NoImageGenerated`、`NoSpeechGenerated`、`NoTranscriptGenerated`、`NoVideoGenerated` → `Output`；`NoSuchProvider`、`NoDefaultRegistry` → `NotFound`。
- 【事实】`Error::is_retryable()` 对 `Provider` 委托 `ProviderError::is_retryable()`，对 `Retry { reason: MaxRetriesExceeded }` 取最后一次错误的可重试性，对 `Stream` 取分片的 `is_retryable`。
