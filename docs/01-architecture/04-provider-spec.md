# Provider 规范层

`ferrin-spec` 定义供应商适配器必须实现的 trait 与数据类型。本层不含任何 HTTP、序列化格式或供应商逻辑。

## 1. 规范版本

【事实】动态类型语言中的 SDK 通常给每个模型接口加规范版本字段，由核心层在运行时把旧版本实例升级到当前版本，以便不同规范版本的适配器共存；这一机制在有编译期类型检查的语言中没有必要。

【决策】Ferrin 不做多规范版本共存。规范版本由 crate 版本表达：`ferrin-spec` 的每个破坏性发布即一次规范升级，供应商 crate 通过 Cargo 版本约束绑定。crate 内导出常量 `pub const SPEC_VERSION: &str = env!("CARGO_PKG_VERSION");` 供诊断输出。依据：Rust 的类型系统在编译期保证适配器与核心使用同一规范；运行时版本字段与升级适配层只在无编译期检查的语言中必要。

## 2. Trait 形态

【决策】每个模型能力定义两层 trait：

1. 实现层 trait：原生 `async fn` 语义，供适配器实现。
2. 对象层 trait：`Dyn` 前缀，返回装箱 Future，对象安全，由 blanket impl 自动提供。

```rust
use std::future::Future;

pub trait LanguageModel: Send + Sync + 'static {
    fn provider(&self) -> &ProviderId;
    fn model_id(&self) -> &ModelId;

    /// URL patterns (by media type) the provider can fetch itself.
    /// Files whose URL does not match are downloaded by the core and inlined.
    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send;

    fn do_generate(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<GenerateResult, ProviderError>> + Send;

    fn do_stream(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<StreamResult, ProviderError>> + Send;
}

pub trait DynLanguageModel: Send + Sync + 'static {
    fn provider(&self) -> &ProviderId;
    fn model_id(&self) -> &ModelId;
    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls>;
    fn do_generate(&self, options: CallOptions) -> BoxFuture<'_, Result<GenerateResult, ProviderError>>;
    fn do_stream(&self, options: CallOptions) -> BoxFuture<'_, Result<StreamResult, ProviderError>>;
}

impl<T: LanguageModel> DynLanguageModel for T { /* boxes each call */ }

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;
```

依据：见[总体架构](01-overall-architecture.md)第 5 节。核心层与中间件持有 `Arc<dyn DynLanguageModel>`；`ferrin_spec::dynamic` 提供 `pub type LanguageModelRef = Arc<dyn DynLanguageModel>` 等别名。

`supported_urls` 为异步。依据：部分供应商需要请求远端能力表才能确定支持的 URL 模式。

## 3. 语言模型

### 3.1 调用选项

【决策】`CallOptions` 覆盖采样参数（最大输出令牌、temperature、top-p、top-k、presence/frequency penalty、停止序列、seed）、响应格式（`text` | `json {schema?, name?, description?}`）、工具与工具选择（`auto` | `none` | `required` | 指定工具）、原始分片开关、取消令牌、请求头、推理等级（`provider-default` | `none` | `minimal` | `low` | `medium` | `high` | `xhigh`）与供应商选项。依据：这是主流供应商采样参数的并集；适配器对不支持的参数产生 `unsupported` 警告而非报错（第 7 节契约第 1 条）。

```rust
#[derive(Debug, Clone)]
pub struct CallOptions {
    pub prompt: Prompt,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
    pub seed: Option<u64>,
    pub response_format: Option<ResponseFormat>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub include_raw_chunks: bool,
    pub reasoning: ReasoningEffort,          // default ProviderDefault
    pub headers: Headers,
    pub provider_options: ProviderOptions,
    pub cancellation: CancellationToken,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponseFormat {
    Text,
    Json { #[serde(default)] schema: Option<JsonValue>, #[serde(default)] name: Option<String>, #[serde(default)] description: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolChoice { Auto, None, Required, Tool { tool_name: ToolName } }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEffort { #[default] ProviderDefault, None, Minimal, Low, Medium, High, XHigh }
```

`CallOptions` 不派生 `Serialize`：它包含取消令牌。fixture 录制使用 `CallOptions::to_recordable()` 输出可序列化快照。

### 3.2 工具定义

【决策】函数工具携带 `name`、`description?`、`input_schema`（JSON Schema）、`strict?`、`input_examples?`、`provider_options?`；供应商工具携带 `id`（`<provider>.<name>`）、`name` 与 `args`（JSON 对象）。依据：函数工具由应用定义并在客户端执行；供应商工具（网页搜索、代码执行等）由供应商定义并在服务端执行，只需要标识与参数。

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolDefinition {
    Function {
        name: ToolName,
        #[serde(default)] description: Option<String>,
        input_schema: JsonValue,
        #[serde(default)] strict: Option<bool>,
        #[serde(default)] input_examples: Vec<JsonObject>,
        #[serde(default)] provider_options: Option<ProviderOptions>,
    },
    Provider {
        id: String,                 // "<provider>.<tool>"
        name: ToolName,
        args: JsonObject,
    },
}
```

### 3.3 结果

【决策】非流式结果携带 `content`、`finish_reason`、`usage`、`provider_metadata?`、`request? {body?}`、`response? {id?, timestamp?, model_id?, headers?, body?}` 与 `warnings`；流式结果携带 `stream`、`request? {body?}` 与 `response? {headers?}`，其余信息以流事件（`response-metadata`、`finish`）传递。依据：流式调用在流结束前无法得知用量与完成原因，这些字段只能作为事件出现。

```rust
pub struct GenerateResult {
    pub content: Vec<Content>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub provider_metadata: Option<ProviderMetadata>,
    pub request: RequestMetadata,        // { body: Option<JsonValue> }
    pub response: ResponseMetadata,      // { id, timestamp, model_id, headers, body }
    pub warnings: Vec<Warning>,
}

pub struct StreamResult {
    pub stream: BoxStream<'static, StreamPart>,
    pub request: RequestMetadata,
    pub response: ResponseMetadata,      // headers only at this point
}
```

【决策】`StreamResult::stream` 的元素类型是 `StreamPart` 而不是 `Result<StreamPart, _>`：供应商错误以 `StreamPart::Error` 事件传递。依据：错误作为事件让核心层的弹性阶段可以在同一条流上实现流级重试与 `on_error` 回调，而不必区分“元素错误”与“流结束原因”。传输层的致命错误（连接中断）由适配器转换为 `Error` 事件后结束流。

### 3.4 流事件顺序契约

【决策】流以 `stream-start` 开始，`finish` 结束；`text-delta` 必须位于同 `id` 的 `text-start`/`text-end` 之间；工具输入以 `tool-input-start`、若干 `tool-input-delta`、`tool-input-end`、`tool-call` 顺序出现；供应商分配的部件 ID 只需在单次调用内唯一，核心层在多步骤流中重映射冲突 ID。依据：明确的开始/结束事件让消费者可以按 `id` 聚合并行部件；ID 唯一性只要求到单次调用，因为供应商的 ID 生成不受核心层控制。

【决策】`ferrin-testing` 提供 `StreamContractChecker`，在适配器测试中断言上述顺序；核心层在 debug 构建下启用同样检查。

## 4. 其他模型接口

下表列出规范层全部接口及其方法。所有方法的完整签名见 `ferrin-spec` 源码文档；此处给出方法与结果的概要。

| Trait | 方法 | 说明 |
| --- | --- | --- |
| `EmbeddingModel` | `max_embeddings_per_call() -> Option<usize>`、`supports_parallel_calls() -> bool`、`do_embed(EmbedOptions{values, cancellation, headers, provider_options}) -> EmbedResult{embeddings, usage{tokens}, provider_metadata, response, warnings}` | 上限由 `max_embeddings_per_call` 给出，核心层据此分块 |
| `ImageModel` | `max_images_per_call() -> Option<usize>`、`do_generate(ImageOptions{prompt, n, size, aspect_ratio, seed, files?, mask?, ...}) -> ImageResult{images, warnings, response, provider_metadata}` | 单次调用可返回多张图像 |
| `SpeechModel` | `do_generate(SpeechOptions{text, voice, output_format, instructions, speed, language, ...}) -> SpeechResult{audio, warnings, request, response, provider_metadata}` | 音频以字节与媒体类型返回 |
| `TranscriptionModel` | `do_generate(TranscriptionOptions{audio, media_type, ...}) -> TranscriptionResult{text, segments, language, duration_in_seconds, ...}`；可选 `do_stream` | 流式转写可选 |
| `RerankingModel` | `do_rerank(RerankOptions{query, documents, top_n, ...}) -> RerankResult{ranking[{index, relevance_score, document?}], usage, ...}` | 结果按相关度降序 |
| `VideoModel` | 同步 `do_generate` 或异步三段式 `do_start`/`do_status`/可选 `handle_webhook` | 两种形态见下文决策 |
| `Files` | `upload_file(UploadFileOptions{data, media_type, filename, provider_options}) -> UploadFileResult{provider_reference, provider_metadata, warnings}`；可选 `get_file_metadata`、`download_file`、`delete_file` | 上传后返回供应商引用，供文件部件使用 |
| `Skills` | `upload_skill(...) -> {provider_reference, ...}` | 与文件上传同构 |
| `Batch` | `start`、`status`、`results`（流）、可选 `cancel`、`list` | 结果按项流式返回 |
| `RealtimeModel` | WebSocket 会话：连接、发送事件、接收标准化事件、客户端密钥获取 | 事件集合见[其他模态](11-other-modalities.md) |
| `SpeechTranslationModel` | 仅流式：`do_stream(...)` | 无非流式形态 |

【决策】`VideoModel` 的同步与异步两种形态在 Rust 中表达为一个 trait：`do_generate`、`do_start`/`do_status`、`handle_webhook` 均有返回 `UnsupportedFunctionality` 的默认实现，并配套 `supports_generate()`、`supports_operations()`、`supports_webhook()` 能力查询；适配器至少实现其中一组。轮询、超时与 Webhook 等待只在核心层 `generate_video` 实现，规范层不含轮询循环（避免 `ferrin-spec` 依赖 `tokio::time`，且轮询策略与取消令牌只需实现一次）。可选方法以 `fn supports_x(&self) -> bool` + 返回 `UnsupportedFunctionality` 错误的默认实现表达，而不是 `Option<fn>`。依据：Rust trait 不能表达可选方法；显式能力查询方法可以让核心层在调用前判断，与动态语言中“方法是否存在”的运行时检查等价。

## 5. Provider trait

【决策】`Provider` trait 以必备方法暴露语言、嵌入与图像模型，转写、语音、重排、文件与技能为可选方法；找不到模型时返回 `NoSuchModelError`。依据：三类必备方法对应全部第一方供应商都提供的能力，其余按供应商可选，缺省实现返回 `None`，核心层据此在调用前判断。

```rust
pub trait Provider: Send + Sync + 'static {
    fn provider_id(&self) -> &ProviderId;

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError>;
    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError>;
    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError>;

    fn transcription_model(&self, model_id: &str) -> Result<TranscriptionModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(self.provider_id(), model_id, ModelKind::Transcription))
    }
    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> { /* same */ }
    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> { /* same */ }
    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> { /* same */ }
    fn speech_translation_model(&self, model_id: &str) -> Result<SpeechTranslationModelRef, NoSuchModelError> { /* same */ }
    fn realtime(&self) -> Option<RealtimeFactoryRef> { None }
    fn files(&self) -> Option<FilesRef> { None }
    fn skills(&self) -> Option<SkillsRef> { None }
    fn batch(&self) -> Option<BatchRef> { None }
}
```

【决策】模型工厂方法同步返回。依据：模型对象在首次请求时才需要凭据；把凭据加载推迟到请求构造阶段，使工厂调用不产生 I/O，也让缺失密钥的错误出现在真正发起调用的地方。

## 6. 规范层错误

【决策】规范层错误以 `ProviderError` 枚举承载（变体见下方代码）；`ApiCallError` 保留 URL、请求体、状态码、响应头、响应体、可重试标志与结构化数据，`is_retryable` 默认在状态码为 408、409、429 或 ≥500 时为真。依据：每个变体对应适配器可能遇到的一类可区分失败，核心层按变体决定重试、降级或直接返回；408/409/429/5xx 是各供应商文档标注为瞬时的状态码。

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    #[error(transparent)] ApiCall(#[from] ApiCallError),
    #[error(transparent)] EmptyResponseBody(#[from] EmptyResponseBodyError),
    #[error(transparent)] InvalidArgument(#[from] InvalidArgumentError),
    #[error(transparent)] InvalidPrompt(#[from] InvalidPromptError),
    #[error(transparent)] InvalidResponseData(#[from] InvalidResponseDataError),
    #[error(transparent)] JsonParse(#[from] JsonParseError),
    #[error(transparent)] LoadApiKey(#[from] LoadApiKeyError),
    #[error(transparent)] LoadSetting(#[from] LoadSettingError),
    #[error(transparent)] NoContentGenerated(#[from] NoContentGeneratedError),
    #[error(transparent)] NoSuchModel(#[from] NoSuchModelError),
    #[error(transparent)] NoSuchProviderReference(#[from] NoSuchProviderReferenceError),
    #[error(transparent)] TooManyEmbeddingValues(#[from] TooManyEmbeddingValuesForCallError),
    #[error(transparent)] TypeValidation(#[from] TypeValidationError),
    #[error(transparent)] UnsupportedFunctionality(#[from] UnsupportedFunctionalityError),
    #[error("operation cancelled")] Cancelled,
    #[error(transparent)] Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ApiCallError {
    pub message: String,
    pub url: Url,
    pub request_body: Option<JsonValue>,
    pub status_code: Option<StatusCode>,
    pub response_headers: Option<Headers>,
    pub response_body: Option<String>,
    pub is_retryable: bool,
    pub data: Option<JsonValue>,
    #[source] pub cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```

`ProviderError::is_retryable()` 仅对 `ApiCall` 变体返回其字段值，其余为 `false`。完整错误模型见[错误模型](12-error-model.md)。

【事实】2026-09-13 实现：`ApiCall`、`InvalidPrompt`、`InvalidResponseData`、`JsonParse`、`NoSuchModel`、`NoSuchProviderReference`、`TooManyEmbeddingValues` 的载荷以 `Box<...>` 承载（`From<具体错误>` 实现自动装箱），`TypeValidationError` 的上下文字段装箱，使 `size_of::<ProviderError>() <= 128`（`clippy.toml` 的 `large-error-threshold`）；测试以 `const_assert!` 固定该上限。

【决策】2026-09-13 增加 `Cancelled` 变体，表示调用被取消令牌中止；`kind_name()` 为 `"cancelled"`，不可重试。依据见 [HTTP 传输与安全](14-http-and-security.md)第 2 节。

## 7. 适配器契约清单

适配器实现必须满足：

1. `do_generate` 与 `do_stream` 在收到不支持的选项时不报错，而是产生 `Warning::Unsupported` 并忽略该选项（例如 Anthropic Messages API 没有 `frequency_penalty`、`presence_penalty` 与 `seed`，适配器对这三项产生警告）。
2. `provider_options` 只读取自身供应商键；键名由供应商配置的 `name` 派生。
3. 工具调用 `input` 原样传递供应商返回的 JSON 字符串，不做解析。
4. 流以 `StreamStart` 开始、`Finish` 结束；错误以 `Error` 事件传递后流结束。
5. `usage.raw` 与 `provider_metadata` 携带供应商原始信息，标准字段无法映射时置 `None`。
6. `request.body` 为发送的 JSON 请求体，`response.body` 为原始响应体（非流式）。
7. 取消令牌触发时中止 HTTP 请求并结束流。
8. 不读取环境变量以外的进程状态；环境变量读取通过 `ferrin_provider_util::settings` 进行。
