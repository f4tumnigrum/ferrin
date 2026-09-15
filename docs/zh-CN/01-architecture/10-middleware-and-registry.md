# 中间件与注册表

[English](../../01-architecture/10-middleware-and-registry.md) | **简体中文**

位于 `ferrin-core::middleware` 与 `ferrin-core::registry`。

## 1. 语言模型中间件

### 1.1 接口

【决策】`LanguageModelMiddleware` 包含六个可选钩子：覆盖供应商 ID、覆盖模型 ID、覆盖支持的 URL、变换调用参数（区分 generate 与 stream）、包裹 `do_generate`、包裹 `do_stream`。依据：前三者让包装后的模型保持正确的标识与能力声明，后三者覆盖输入变换与输出包裹两类需求。

```rust
pub trait LanguageModelMiddleware: Send + Sync + 'static {
    fn override_provider(&self, model: &dyn DynLanguageModel) -> Option<ProviderId> { None }
    fn override_model_id(&self, model: &dyn DynLanguageModel) -> Option<ModelId> { None }
    fn override_supported_urls(&self, model: &dyn DynLanguageModel) -> Option<BoxFuture<'static, SupportedUrls>> { None }

    fn transform_params(
        &self,
        params: CallOptions,
        call_type: CallType,                          // Generate | Stream
        model: &dyn DynLanguageModel,
    ) -> BoxFuture<'_, Result<CallOptions, Error>> { Box::pin(async move { Ok(params) }) }

    fn wrap_generate(
        &self,
        next: ModelCall<'_>,                          // { do_generate(), do_stream(), params, model }
    ) -> BoxFuture<'_, Result<GenerateResult, ProviderError>> { next.do_generate() }

    fn wrap_stream(
        &self,
        next: ModelCall<'_>,
    ) -> BoxFuture<'_, Result<StreamResult, ProviderError>> { next.do_stream() }
}
```

`ModelCall` 同时暴露 `do_generate` 与 `do_stream`，使得 `wrap_stream` 可以改用非流式调用（模拟流中间件依赖此能力）。

### 1.2 组合顺序

【决策】`wrap_language_model(model, [a, b, c])` 先反转数组再依次包裹，结果是 `a` 最先转换输入、`c` 直接贴近模型；`model_id`/`provider_id` 参数可覆盖包装后的标识。依据：按书写顺序“先外后内”符合读者对中间件链的直觉。

```rust
pub fn wrap_language_model(
    model: impl Into<LanguageModelRef>,
    middleware: impl IntoIterator<Item = Arc<dyn LanguageModelMiddleware>>,
) -> LanguageModelRef;

pub struct WrapOptions { pub model_id: Option<ModelId>, pub provider_id: Option<ProviderId> }
pub fn wrap_language_model_with(model: impl Into<LanguageModelRef>, middleware: ..., options: WrapOptions) -> LanguageModelRef;
```

包装结果实现 `DynLanguageModel`，`provider()` 与 `model_id()` 按“显式覆盖 > 中间件 override > 原模型”解析。

### 1.3 内置中间件

【决策】内置中间件：

| 中间件 | 行为 |
| --- | --- |
| `default_settings` | 参数变换阶段把默认设置与调用参数深合并，调用参数优先。 |
| `extract_reasoning`（`tag_name`、`separator` 默认 `\n`、`start_with_reasoning` 默认 false） | 从文本中提取 `<tag>...</tag>` 为推理部件；流式实现按标签边界切分，保证 `text-start` 不早于 `reasoning-start` 发出，空推理块也发出 `reasoning-start`/`reasoning-end`。 |
| `simulate_streaming` | 包裹 `do_stream` 时调用 `do_generate`，把结果展开为 `stream-start`、`response-metadata`、逐部件的 start/delta/end、`finish`。 |
| `extract_json` | 从文本中提取 JSON 代码块作为响应。 |
| `add_tool_input_examples` | 把工具的 `input_examples` 追加到工具描述中，供不支持示例字段的供应商使用。 |

实现位于 `ferrin_core::middleware::builtin`：`default_settings`、`extract_reasoning`、`simulate_streaming`、`extract_json`、`add_tool_input_examples`。

### 1.4 嵌入模型与图像模型中间件

【决策】`ImageModelMiddleware` 与 `wrap_image_model` 提供参数变换与 `do_generate` 包裹；`wrap_provider` 可同时对供应商的全部语言模型与图像模型应用中间件。

【决策】`EmbeddingModelMiddleware` 与 `wrap_embedding_model` 为嵌入模型提供同样的能力：`transform_params(EmbedOptions)`、`wrap_embed`、`override_provider`、`override_model_id`，以及限制钩子 `max_embeddings_per_call`、`max_input_bytes_per_call`、`supports_parallel_calls`——它们接收被包裹的模型，默认转发其原值。依据：`embed_many` 正是按这些限制切分与调度，做批处理或代理的中间件必须能改写它们；转发式默认值让不做改写的层保持透明。图像侧以同样方式暴露 `max_images_per_call`。两个 trait 其余部分与第 1.1 节一致（直通默认实现、`BoxFuture` 返回值、续延类型 `EmbedNext` / `ImageGenerateNext`），组合顺序遵循第 1.2 节。

【决策】`wrap_provider(provider, ProviderMiddleware { language_model, embedding_model, image_model })` 包裹供应商解析出的这三类模型中的每一个，保留供应商 ID，其余模型种类与服务（`realtime`、`files`、`skills`、`batch`）原样委托。有意纳入嵌入模型：否则无法按供应商表达注册表级的嵌入默认值（第 1.3 节）。空的 `ProviderMiddleware` 直接返回原供应商。供应商若返回未解析的模型 ID，则无法包裹，报 `NoSuchModel` 并附说明。

【决策】`default_embedding_settings(EmbeddingDefaults { headers, provider_options })` 是 `default_settings` 的嵌入版：请求头与供应商选项与调用值合并，调用值优先，供应商选项经 `merge_json_objects` 递归合并。

## 2. 注册表

### 2.1 ProviderRegistry

【决策】供应商注册表（分隔符默认 `:`，可附加语言模型、嵌入模型与图像模型中间件）：

- `language_model("openai:gpt-5")` 按首个分隔符拆分为供应商 ID 与模型 ID；缺少分隔符报 `NoSuchModel` 错误，供应商不存在报 `NoSuchProvider` 错误（携带供应商 ID 与可用列表），供应商返回空报 `NoSuchModel` 错误。
- 注册表级中间件应用到每个取出的语言模型、嵌入模型与图像模型。
- 支持 `embedding_model`、`image_model`、`transcription_model`、`speech_model`、`reranking_model`、`video_model`、`files(provider_id)`、`skills(provider_id)`。

```rust
pub struct ProviderRegistry { /* ... */ }

impl ProviderRegistry {
    pub fn builder() -> ProviderRegistryBuilder;
    pub fn language_model(&self, id: &str) -> Result<LanguageModelRef, Error>;
    pub fn embedding_model(&self, id: &str) -> Result<EmbeddingModelRef, Error>;
    pub fn image_model(&self, id: &str) -> Result<ImageModelRef, Error>;
    pub fn transcription_model(&self, id: &str) -> Result<TranscriptionModelRef, Error>;
    pub fn speech_model(&self, id: &str) -> Result<SpeechModelRef, Error>;
    pub fn reranking_model(&self, id: &str) -> Result<RerankingModelRef, Error>;
    pub fn video_model(&self, id: &str) -> Result<VideoModelRef, Error>;
    pub fn files(&self, provider_id: &str) -> Result<FilesRef, Error>;
    pub fn skills(&self, provider_id: &str) -> Result<SkillsRef, Error>;
}

impl ProviderRegistryBuilder {
    pub fn provider(self, id: impl Into<ProviderId>, provider: Arc<dyn Provider>) -> Self;
    pub fn separator(self, sep: impl Into<String>) -> Self;
    pub fn language_model_middleware(self, mw: Arc<dyn LanguageModelMiddleware>) -> Self;
    pub fn embedding_model_middleware(self, mw: Arc<dyn EmbeddingModelMiddleware>) -> Self;
    pub fn image_model_middleware(self, mw: Arc<dyn ImageModelMiddleware>) -> Self;
    pub fn build(self) -> ProviderRegistry;
}
```

`ProviderRegistry` 自身实现 `Provider`，可嵌套。

### 2.2 CustomProvider

【决策】`custom_provider` 用别名映射预配置模型实例（语言、嵌入、图像等模型以及文件与技能接口），未命中时委托给回退供应商，否则报 `NoSuchModel` 错误。依据：别名让应用以语义名（`fast`、`smart`）引用模型，切换供应商时只需改映射。

```rust
pub fn custom_provider() -> CustomProviderBuilder;

impl CustomProviderBuilder {
    pub fn language_model(self, alias: impl Into<ModelId>, model: impl Into<LanguageModelRef>) -> Self;
    pub fn embedding_model(...) -> Self;
    pub fn image_model(...) -> Self;
    // transcription, speech, reranking, video
    pub fn files(self, files: FilesRef) -> Self;
    pub fn skills(self, skills: SkillsRef) -> Self;
    pub fn fallback(self, provider: Arc<dyn Provider>) -> Self;
    pub fn build(self) -> Arc<dyn Provider>;
}
```

### 2.3 模型引用与字符串解析

【事实】以字符串指定模型并经进程级全局默认供应商解析的做法，会让未显式配置的调用发起网络请求，且解析结果依赖全局可变状态。

【决策】Ferrin 的模型参数类型为 `impl Into<LanguageModelRef>`；为字符串提供 `LanguageModelRef::from_id("openai:gpt-5")` 时，解析依赖显式设置的进程级默认注册表：

```rust
pub fn set_default_registry(registry: ProviderRegistry) -> Result<(), DefaultRegistryAlreadySet>;
pub fn default_registry() -> Option<&'static ProviderRegistry>;
```

未设置时以字符串引用发起调用返回 `Error::NoDefaultRegistry`。依据：项目范围排除隐式网关（[项目定位与范围](../00-overview/01-project-scope.md)）；`OnceLock` 单次设置避免运行期竞态。

## 3. 示例

```rust
let registry = ProviderRegistry::builder()
    .provider("openai", ferrin_openai::create_openai(Default::default())?)
    .provider("anthropic", ferrin_anthropic::create_anthropic(Default::default())?)
    .language_model_middleware(Arc::new(default_settings(CallDefaults { temperature: Some(0.2), ..Default::default() })))
    .embedding_model_middleware(Arc::new(default_embedding_settings(EmbeddingDefaults { headers: Headers::new().with("x-team", "search"), ..Default::default() })))
    .build();

let model = registry.language_model("anthropic:claude-sonnet-4-5")?;
let wrapped = wrap_language_model(model, [Arc::new(extract_reasoning("think")) as Arc<dyn LanguageModelMiddleware>]);
```

## 4. 待验证

- 【决策】（PV-010）`extract_reasoning` 的最低测试覆盖为 14 个用例。非流式 5 个：提取 `<think>` 标签、无正文时提取、多个标签、`start_with_reasoning` 为真时前置标签、保留其他属性。流式 9 个：缺失 id 的分片不崩溃、标签跨分片、单分片多标签、无正文时提取、`start_with_reasoning` 前置、无标签时保留原文、空 `<think></think>` 不崩溃，以及对应的 id 与事件顺序断言。依据：这些用例覆盖标签边界与分片边界的全部组合，是该中间件最容易出错的地方。
- 【决策】`ferrin-core` 的 `extract_reasoning` 单元测试以上述 14 个用例为最低集合，测试名与参考用例一一对应（`tests/suite/middleware/extract_reasoning.rs`），实现 PR 必须附带全部用例后方可合并。

## 5. 实现记录（2026-09-13）

- 【事实】`ferrin_core::middleware::builtin` 除第 1.3 节所列五个中间件外提供 `default_instructions(instructions)`：当调用未带系统消息时在 prompt 最前插入默认指令。`extract_reasoning(tag_name)` 提供 `separator`、`start_with_reasoning` 配置；`add_tool_input_examples()` 提供 `prefix`、`format`、`remove`；`extract_json()` 提供 `transform`；`default_settings(CallDefaults)` 的 `provider_options` 以对象深合并（`merge_json_objects`），调用参数优先。
- 【决策】`ProviderRegistry` 作为 `Provider` 时的 `provider_id()` 为 `"registry"`。依据：嵌套注册表需要一个稳定标识以便在 `NoSuchProvider` 错误与遥测中区分层级。
- 【事实】`ProviderRegistry::realtime_model(id)` 通过供应商的 `realtime()` 工厂解析实时模型；供应商不支持实时会话时返回 `NoSuchModel`（`ModelKind::Realtime`）。第 2.1 节的 `files(provider_id)`、`skills(provider_id)` 与 `speech_translation_model(id)` 同样提供。
- 【决策】进程级默认注册表通过 `registry::set_default_registry()`（`OnceLock`，只能设置一次，重复设置返回 `Error::InvalidArgument { argument: "registry" }`）配置；未设置时以字符串形式传入的模型 ID 返回 `Error::NoDefaultRegistry`。

【事实】 流式推理提取在文本结束、Finish 和流结束时按字面保留未完成的标签前缀。所有提取块（包括连续空块及未闭合块）均有配对的开始/结束事件，推理 ID 在不同源文本块之间保持唯一（2026-09-15，`tests/suite/middleware/extract_reasoning.rs`）。

## 6. 实现记录（2026-09-15）

- 【事实】`ferrin_core::middleware` 按第 1.4 节提供 `EmbeddingModelMiddleware` / `wrap_embedding_model`（`embedding.rs`）、`ImageModelMiddleware` / `wrap_image_model`（`image.rs`）与 `ProviderMiddleware` / `wrap_provider`（`provider.rs`）；`builtin::default_embedding_settings` 为嵌入版默认设置中间件。包装器在包裹时一次性确定 `provider()` / `model_id()`（先中间件覆盖，后内层模型），限制钩子在每次调用时求值。
- 【事实】注册表构建器每次调用附加一个中间件（`language_model_middleware(Arc<dyn _>)`、`embedding_model_middleware`、`image_model_middleware`），按最外层优先的顺序追加；`ProviderRegistry::embedding_model` 与 `image_model` 与 `language_model` 一样应用各自的列表。供应商返回未解析引用时报 `Error::NoDefaultRegistry`。
- 【事实】覆盖：`tests/suite/middleware/{embedding,image,provider,default_embedding_settings}.rs`（组合顺序、经 `embed_many` / `generate_image` 分块观察到的标识与限制覆盖、供应商直通、未解析引用、请求头与供应商选项优先级）与 `tests/suite/registry.rs`（注册表嵌入/图像中间件）。

【决策】 语言模型中间件对工具的过滤也约束本地工具执行。每次模型调用尝试拥有独立的工具约束：各层只能收窄工具名称集合；解析调用或校验结束条件前，按集合的交集规范化最终工具选择。约束通过带作用域的中间件续体传递（包括在子任务中轮询的续体），流消费期间持续保留，重试时重新创建。这使策略过滤真正生效，无须向供应商规范新增字段，也不在并发调用间共享可变状态。
