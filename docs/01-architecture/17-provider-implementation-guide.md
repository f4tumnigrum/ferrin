# Provider 适配器实现指南

本指南面向供应商 crate 的实现者，描述 `ferrin-openai`、`ferrin-anthropic` 等 crate 的统一结构与必须遵守的契约。

## 1. 适配器的通用结构

【决策】每个供应商 crate 遵循同一结构：

- 导出 `create_xxx(settings)` 工厂与 `XxxProvider` 类型；后者实现 `Provider` trait，并提供按 API 族命名的方法（`chat`、`responses`、`completion`、`embedding`、`image`、`transcription`、`speech`、`files`、`skills`、`batch`、`realtime`）以及 `tools` 模块（供应商工具工厂）。
- 设置：`base_url`（环境变量兜底，校验并去除尾部斜杠，Anthropic 会把裸域名规范化为 `/v1`）、`api_key`（环境变量兜底，惰性加载）、`headers`、`name`（覆盖供应商名以支持第三方兼容端点）、`transport`、`id_generator`。
- 每个模型类型接收 `(model_id, config)`，`config` 包含供应商 ID 字符串（`<name>.<family>`，如 `openai.chat`）、URL 构造、请求头构造与传输。
- `build_request` 把 `CallOptions` 转换为请求体并收集警告：不支持的参数产生 `unsupported` 警告；`provider_options` 按供应商键反序列化；工具经工具准备转换并返回工具警告。
- `do_generate` 使用 JSON 请求辅助与 JSON 响应处理器；`do_stream` 使用 SSE 响应处理器并经 `stream_driver` 把供应商事件映射为规范流事件，首个事件为 `stream-start {warnings}`。
- 错误处理器由 JSON 错误响应处理器（错误体类型 + 消息提取）构造；流内错误以 `StreamError` 携带类型推断的状态码与可重试性。
- 完成原因映射为统一枚举并保留原始值；用量映射到标准结构并保留 `raw`。

## 2. Crate 骨架

```rust
// ferrin-openai/src/lib.rs（2026-09-13 实现的签名）
pub struct OpenAiSettings {
    pub base_url: Option<Url>,                       // env OPENAI_BASE_URL
    pub api_key: Option<SecretString>,               // env OPENAI_API_KEY (lazy)
    pub organization: Option<String>,
    pub project: Option<String>,
    pub headers: Headers,
    pub name: Option<String>,                        // provider name override
    pub transport: Option<SharedTransport>,
    pub id_generator: Option<Arc<dyn IdGenerator>>,
}

pub fn create_openai(settings: OpenAiSettings) -> Result<OpenAiProvider, ProviderError>;

impl OpenAiProvider {
    pub fn from_config(config: SharedConfig) -> Self;
    pub fn config(&self) -> &SharedConfig;
    pub fn responses(&self, model_id: impl Into<ModelId>) -> OpenAiResponsesLanguageModel;
    pub fn chat(&self, model_id: impl Into<ModelId>) -> OpenAiChatLanguageModel;
    pub fn completion(&self, model_id: impl Into<ModelId>) -> OpenAiCompletionLanguageModel;
    pub fn embedding(&self, model_id: impl Into<ModelId>) -> OpenAiEmbeddingModel;
    pub fn image(&self, model_id: impl Into<ModelId>) -> OpenAiImageModel;
    pub fn speech(&self, model_id: impl Into<ModelId>) -> OpenAiSpeechModel;
    pub fn transcription(&self, model_id: impl Into<ModelId>) -> OpenAiTranscriptionModel;
    #[cfg(feature = "realtime")]
    pub fn speech_translation(&self, model_id: impl Into<ModelId>) -> OpenAiSpeechTranslationModel;
    pub fn files(&self) -> OpenAiFiles;
    pub fn skills(&self) -> OpenAiSkills;
    pub fn batch(&self) -> OpenAiBatch;
    pub fn realtime(&self) -> OpenAiRealtimeFactory;
    pub fn tools(&self) -> &OpenAiTools;             // provider-defined / provider-executed tool factories
}

impl Provider for OpenAiProvider { /* language_model = responses(), 其余返回对应的 Ref */ }
```

【决策】`create_openai` 返回 `Result`：`base_url` 无效时立即失败；凭据缺失不在此处失败（惰性加载）。依据：URL 错误是配置错误，越早失败越好；凭据可能由请求级头部覆盖。

【决策】按 API 族命名的方法返回具体模型类型而不是 `XxxRef`：具体类型暴露 `prepare_request`、`config` 等供测试与组合使用，需要动态分发时由调用方 `into()` 为 Ref。依据：`Provider` trait 已提供 Ref 形态，具体类型不增加公共 API 面以外的成本。

## 3. 语言模型实现步骤

1. 定义 `api_types.rs`：请求体（`Serialize`）、响应体与流分片（`Deserialize`，未知字段忽略），字段名保持供应商原样（`#[serde(rename_all = "snake_case")]` 或逐字段 `rename`）。
2. 定义 `options.rs`：供应商选项结构体（`Deserialize + JsonSchema`），通过 `parse_provider_options::<OpenAiResponsesOptions>("openai", &options.provider_options)` 解析。
3. 实现 `convert_prompt.rs`：`spec::Prompt -> Vec<ApiMessage>`。规则：
   - 文件部件的 `FileData::Reference` 通过 `resolve_provider_reference(reference, provider_key)` 取 ID；不支持时返回 `UnsupportedFunctionality`。
   - `FileData::Url` 仅当该 URL 匹配 `supported_urls` 时直传，否则核心层已内联为字节。
   - 工具结果输出按供应商格式映射：`Text`/`Json` → 字符串或结构化内容；`ErrorText`/`ErrorJson` → 带错误标记；`ExecutionDenied` → 文本说明；`Content` → 多模态内容数组。
   - 不支持的部件（如某些供应商不接受助手侧文件）产生警告并跳过，而不是报错。
4. 实现 `convert_tools.rs`：`ToolDefinition::Function` → 供应商函数工具；`ToolDefinition::Provider { id }` 只接受本供应商前缀的 ID，其他产生 `Unsupported` 警告；工具名不合法时使用 `ToolNameMapping` 重命名并在响应中还原。
5. 实现 `build_request()`：收集警告；把 `ReasoningEffort` 通过 `map_reasoning_to_effort` 或 `map_reasoning_to_budget` 映射（【决策】effort 映射表缺失产生 `unsupported`，映射到不同值产生 `compatibility`；budget 映射按 `max_output_tokens` 百分比 `minimal 2%`、`low 10%`、`medium 30%`、`high 60%`、`xhigh 90%`，下限 1024。依据：Anthropic 的思考预算以令牌数表达，按输出上限的比例换算使同一 `ReasoningEffort` 在不同 `max_output_tokens` 下保持相对强度；1024 是 Anthropic 文档规定的最小预算）。
6. 实现 `do_generate`：`post_json` + `json_response_handler`，映射内容、完成原因、用量、`provider_metadata`，填充 `request.body` 与 `response.{id, timestamp, model_id, headers, body}`。
7. 实现 `do_stream`：`post_json` + `event_source_response_handler`，以 `async_stream` 或手写 `Stream` 把分片映射为 `StreamPart`：先发 `StreamStart { warnings }`，遇到首个含元数据的分片发 `ResponseMetadata`，维护文本/推理/工具输入的 start/delta/end 状态机，结束时发 `Finish`；解析失败的分片转为 `Error` 事件；`include_raw_chunks` 为真时在每个分片前发 `Raw`。
8. 实现 `map_finish_reason.rs` 与 `convert_usage.rs`。
9. 实现错误处理器：错误响应 schema、消息提取、状态码与可重试性覆盖。

## 4. 供应商工具

【决策】各供应商 crate 的 `tools` 模块是工具工厂，返回供应商定义/供应商执行工具，携带 `id`（如 `openai.web_search`）、`args`、输入输出 schema，以及供应商特定的输出解析。依据：供应商工具的参数与输出形状由供应商文档定义，工厂函数把这些知识收敛在适配器内。

```rust
impl OpenAiTools {
    pub fn web_search(&self, config: WebSearchConfig) -> Tool;      // ProviderExecuted
    pub fn file_search(&self, config: FileSearchConfig) -> Tool;    // ProviderExecuted
    pub fn code_interpreter(&self, config: CodeInterpreterConfig) -> Tool;
    pub fn image_generation(&self, config: ImageGenerationConfig) -> Tool;
}
```

## 5. 测试要求

【决策】供应商测试以 fixture 驱动：`tests/fixtures/<area>/<case>.chunks.txt` 保存原始 SSE 分片（每行一个 `data:` 事件），`<case>.response.json` 保存非流式响应；测试用录制的分片重建 SSE 响应并对规范流事件做快照断言。依据：见[测试规范](../03-engineering/04-testing.md)第 1 节。

供应商 crate 必须包含：

1. 每个 API 族至少一个非流式与一个流式 fixture 用例，覆盖文本、工具调用、推理、错误、用量。
2. `StreamContractChecker` 断言（`ferrin-testing`），保证事件顺序契约。
3. 请求体快照（`insta`）：验证 `build_request` 对每种选项组合的输出。
4. 警告断言：不支持参数产生预期警告。
5. 可选的 `#[ignore]` 在线测试，读取环境变量中的密钥（`live_*` 命名，以供应商密钥环境变量门控）。

fixture 录制通过 `cargo xtask record-fixture --provider openai --case responses/tool-call` 执行，使用真实密钥调用一次并写入文件，密钥与账户信息在写入前从响应头中剔除。

## 6. `ferrin-openai-compatible`

【决策】`ferrin-openai-compatible` 提供通用 Chat/Completion/Embedding/Image 模型类型，接受供应商名、URL、请求头、传输与是否支持结构化输出等配置，供第三方端点（如 DeepSeek、Together）与专用供应商 crate 复用。依据：大量端点只宣称 Chat Completions 兼容，通用实现加配置开关比为每个端点写适配器更经济。

【事实】`ferrin-openai-compatible` 导出 `OpenAiCompatibleProvider` 与 `create_openai_compatible(OpenAiCompatibleSettings { name, base_url, api_key, api_key_env, headers, query_params, include_usage, supports_structured_outputs, supported_urls, error_structure, metadata_extractor, transform_request_body, convert_usage, max_embeddings_per_call, supports_parallel_calls, transport, id_generator })`，以及可被其他 crate 组合的模型类型 `OpenAiCompatibleChatLanguageModel`、`OpenAiCompatibleCompletionLanguageModel`、`OpenAiCompatibleEmbeddingModel`、`OpenAiCompatibleImageModel`（共享 `OpenAiCompatibleConfig`）与扩展 trait `ErrorStructure`、`MetadataExtractor`/`StreamMetadataExtractor`（2026-09-13 按实现更正类型名与设置列表；实现记录见第 11 节，能力与选项见 [OpenAI 兼容端点](../providers/openai-compatible.md)）。

## 7. 新增供应商检查清单

- [ ] crate 名 `ferrin-<provider>`，`Cargo.toml` 继承工作区 lints 与版本。
- [ ] `create_<provider>()` 与设置结构体；环境变量名在文档中列出。
- [ ] 实现 `Provider` trait；不支持的模型类型使用默认实现。
- [ ] 所有网络访问经 `ferrin_provider_util::http`。
- [ ] User-Agent 后缀 `ferrin-<provider>/<version>`。
- [ ] fixture 测试、请求体快照、契约检查。
- [ ] `docs/providers/<provider>.md` 记录：支持的能力矩阵、供应商选项 schema、供应商元数据字段、已知限制与警告。
- [ ] 在 `ferrin` 门面 crate 增加 feature 与 re-export。
- [ ] 变更日志条目。

## 8. 待验证

- 【决策】（PV-021）`OpenAiConfig` 保留两个面向兼容端点的开关：`explicit_message_item_type`（Azure Foundry 项目端点的 Responses 输入消息需要显式 `type: 'message'`）与 `supports_web_search_sources_include`（Amazon Bedrock Mantle 端点不接受 `web_search_call.action.sources` 的 include 值）。依据：这两类端点复用 OpenAI Responses 的请求形状但存在已知差异，以配置开关表达比分叉实现更易维护。
- 【决策】`ferrin-openai` 的 `OpenAiConfig` 保留这两个字段（`explicit_message_item_type: bool`，默认 `false`；`supports_web_search_sources_include: bool`，默认 `true`），`ferrin-openai-compatible` 在 Responses 模式下透传；这是支持 Azure Foundry 与 Bedrock Mantle 类端点的必要开关。（2026-09-13 修订：`ferrin-openai-compatible` 不提供 Responses 模式，透传条款撤销，见 [ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md)；两个字段仍保留在 `OpenAiConfig` 中，兼容端点的 Responses 调用使用 `ferrin-openai` 的 `base_url` 与 `name` 设置。）
## 9. 实现记录（2026-09-13，`ferrin-openai`）

- 【事实】模块布局：`config`（`OpenAiConfig`/`SharedConfig`、URL 与头部组装、`websocket_url`）、`capabilities`（按模型 ID 推断推理模型、系统消息模式、`flex`/`priority` 支持）、`responses/{options, api_types, convert_prompt, convert_tool_results, convert_tools, request, output, stream, stream/items}`、`chat/*`、`completion`、`embedding`、`image`、`speech`、`transcription`（含 `realtime_stream`）、`speech_translation`（feature `realtime`）、`realtime`、`files`、`skills`、`batch/{api_types, results}`、`tools`、`error`、`stream_util`、`realtime_ws`（feature `realtime`）。每个文件不超过 800 行（`cargo xtask check-module-size`）。
- 【决策】供应商 ID 为 `<name>.<family>`（`openai.responses`、`openai.chat`、`openai.completion`、`openai.embedding`、`openai.image`、`openai.speech`、`openai.transcription`、`openai.speech-translation`、`openai.realtime`、`openai.batch`、`openai.files`、`openai.skills`）；`name` 覆盖时前缀随之变化，`provider_options` 的键（`OpenAiConfig::provider_options_key`，默认 `openai`）与文件引用键（`config.name`）分别配置，键不同且该键下无选项时回退读取 `openai` 键。依据：第三方兼容端点复用本 crate 时既要区分供应商 ID，又要接受按 `openai` 键编写的选项。
- 【决策】三个流式语言模型共用 `ferrin_provider_util::stream_driver::drive_stream`（实现 `ferrin-anthropic` 时从本 crate 的 `stream_util` 提升到 `ferrin-provider-util`，`stream_util` 保留再导出、OpenAI 错误映射与格式化辅助函数）：实现 `StreamMachine`（`handle(chunk) -> Vec<StreamPart>`、`finish()`）的状态机由 `futures_util::stream::unfold` 驱动；驱动器跟踪未关闭的 text/reasoning/tool-input 部件，遇到状态机产出的 `StreamPart::Error` 时先补发对应的 end 部件，再转发错误并结束流，不再产出 `Finish`。依据：核心层 `stream_text` 收到 `Error` 即结束本次尝试（重试或报错），`StreamContractChecker` 也把 `Error` 视为终止部件，`Error` 之后的 `Finish` 永远不会被消费。
- 【决策】`stream_util::fail_on_early_error` 在把流交给调用方之前读取开头的分片：服务器在产出任何输出之前返回错误帧（Responses 的 `error`/`response.failed`、Chat/Completions 的 `{"error": ...}`）时，`do_stream` 以 `ProviderError::ApiCall`（状态码由错误码推断）失败而不是返回只含错误事件的流；收到 `response.in_progress` 后最多再等待 50 ms 的输出，已读分片原样重放。依据：核心层的重试策略依赖 `ApiCall` 错误的状态码与可重试性，流内错误事件不参与请求级重试。
- 【事实】WebSocket 模型（流式转写、语音翻译）通过子协议 `["realtime", "openai-insecure-api-key.<key>"]` 认证并从请求头中移除 `authorization`；URL 由 `OpenAiConfig::websocket_url` 从 `base_url` 派生（`https`→`wss`，`http`→`ws`）。不启用 `realtime` feature 时 `TranscriptionModel::supports_stream` 返回 `false`，`Provider::speech_translation_model` 返回带提示的 `NoSuchModelError`。
- 【事实】测试位于 `tests/suite/*.rs`（`tests/all.rs` 汇总），fixture 位于 `tests/fixtures/<area>/`，流式用例以 `-stream` 后缀区分；WebSocket 测试用 `tokio-tungstenite` 起本地服务器回显子协议并记录会话消息。fixture 为手工编写（PV-031）。
- 【事实】第 5 节提到的 `cargo xtask record-fixture` 于 2026-09-14 实现（见[工作区布局](../03-engineering/02-workspace-layout.md)第 6 节）；本 crate 的 fixture 尚未用它重新录制（PV-031）。

## 10. 实现记录（2026-09-13，`ferrin-anthropic`）

- 【事实】模块布局：`config`（`AnthropicConfig`/`SharedConfig`/`Credential`、URL、头部与 beta 合并）、`capabilities`（按模型 ID 推断 `max_tokens` 上限、结构化输出、自适应思考、采样参数与 `xhigh` 支持）、`options`（语言模型、部件、系统消息、工具与推理元数据的选项 schema）、`api_types`、`error`（HTTP 错误体与流内 `error` 事件的映射）、`cache_control`（断点计数与位置校验）、`json_schema`（`sanitize_json_schema`）、`usage`、`convert_prompt/{mod, user, assistant, provider_results}`、`prepare_tools`、`request/{mod, validate, body}`、`output/{mod, results, metadata}`、`stream`、`messages`、`tools`、`files`、`skills`、`batch/{mod, results}`、`path`。最大文件 613 行（`cargo xtask check-module-size`）。
- 【决策】供应商 ID 为 `<name>.<family>`（`anthropic.messages`、`anthropic.batch`、`anthropic.files`、`anthropic.skills`；`AnthropicProvider::chat` 是 `messages` 的别名）。`provider_options` 始终读取 `anthropic` 键；`name` 不同时再读取 `name` 键并以其覆盖，结果级 `provider_metadata` 同时写入两个键；文件与技能引用键为 `name`。依据：兼容端点或多实例场景复用本 crate 时既要区分供应商 ID，又要接受按 `anthropic` 键编写的选项与元数据读取代码。
- 【决策】`AnthropicConfig` 提供 `supports_strict_tools`（默认 `true`）与 `supports_native_structured_output`（默认 `true`）两个开关：前者为 `false` 时忽略函数工具的 `strict` 并警告，后者为 `false` 时结构化输出一律回退为 `json` 工具。依据：通过第三方平台（如云厂商托管的 Claude 端点）访问 Anthropic 模型时，这两项能力可能不可用，兼容端点需要能关闭它们。
- 【决策】流式实现复用 `ferrin_provider_util::stream_driver`：`AnthropicStreamState` 实现 `StreamMachine`，以内容块索引作为部件 ID，`message_start` 中预填的 `tool_use` 块立即产出完整的 tool-input 与 tool-call 部件，`content_block_stop` 时补齐空输入 `{}`；`fail_on_early_error` 把首个事件为 `error` 的流转换为 `ProviderError::ApiCall`（状态码按 `error.rs` 的类型表推断），其余错误事件作为终止的 `StreamPart::Error` 产出。依据：与第 9 节相同，核心层的重试依赖请求级 `ApiCall` 错误，流内错误不参与重试。
- 【决策】`json` 工具回退：工具名固定为 `json`、描述 `Respond with a JSON object.`，`tool_choice: {type: any, disable_parallel_tool_use: true}`，不追加 `structured-outputs-2025-11-13` beta；批处理请求拒绝该回退（`UnsupportedFunctionality`），因为批次结果按原始工具名转换，无法区分回退工具。依据：批处理结果转换不持有请求时的工具映射。
- 【事实】`anthropic-beta` 头由配置头、调用头与请求推导出的 beta 合并（去重、小写、排序、逗号连接），用户提供的 `anthropicBeta` 选项与批次级 `anthropicBeta` 同样并入；每项能力对应的 beta 见 `docs/providers/anthropic.md`。
- 【事实】`max_tokens` 缺省为模型上限；预算式思考时加上 `budget_tokens`，超过模型上限时截断，只有用户显式设置 `maxOutputTokens` 时才警告；未知的 `claude-*` ID 按最新能力处理并产生 `maxOutputTokens` 兼容性警告。
- 【事实】测试位于 `tests/suite/*.rs`（`tests/all.rs` 汇总，58 个用例），fixture 位于 `tests/fixtures/<area>/`；`.chunks.txt` 每行一个已编码的 `event:`/`data:` 事件（`ferrin_testing::encode_events_file` 格式）。fixture 为手工编写（PV-031）。
- 【事实】第 5 节提到的 `cargo xtask record-fixture` 于 2026-09-14 实现（见[工作区布局](../03-engineering/02-workspace-layout.md)第 6 节）；本 crate 的 fixture 尚未用它重新录制（PV-031）。

## 11. 实现记录（2026-09-13，`ferrin-openai-compatible`）

- 【事实】模块布局：`config`（`OpenAiCompatibleConfig`/`SharedConfig`、URL 与查询参数、头部与 Bearer 凭据、钩子类型 `TransformRequestBody`/`ConvertUsage`）、`options_key`（选项键解析：`to_camel_case`、`option_keys`、`merged_options`、`passthrough_options`、`resolve_metadata_key`、`warn_if_deprecated_key`、`shared_extra_fields`）、`error`（`ErrorStructure` trait、`DefaultErrorStructure`、HTTP 错误处理器与流内错误帧的状态推断）、`metadata`（`MetadataExtractor`/`StreamMetadataExtractor` trait 与合并工具）、`chat/{mod, api_types, options, prepare_tools, convert_prompt, output, stream}`、`completion`、`embedding`、`image`。最大文件 554 行（`cargo xtask check-module-size`）。
- 【决策】模型族只有 `<name>.chat`、`<name>.completion`、`<name>.embedding`、`<name>.image`，不提供 Responses 模式（[ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md)）。`Provider::language_model` 返回 Chat 族。
- 【决策】选项键按 `openai-compatible`（已弃用，警告）→ `openaiCompatible` → `<name>` → camelCase(`<name>`) 的顺序合并，后者覆盖前者；`provider_metadata` 写入调用方使用的 camelCase 键，否则写入 `<name>`；以非 camelCase 的 `<name>` 提供选项时产生 `deprecated` 警告。`to_camel_case` 只替换后跟 ASCII 小写字母的 `_`/`-`。依据：兼容端点的选项键通常以 JavaScript 习惯的 camelCase 书写，同时接受 `<name>` 原样与 camelCase 形式使配置来源无需转换。
- 【决策】只有 `<name>`/camelCase 键下未被 schema 消费的字段透传到请求体（Chat、Completions、图像），`openaiCompatible` 键下的字段不透传；消息与部件级只读取 `openaiCompatible` 键并把对象展开到线格式对象。依据：共享键承载跨端点通用的选项，端点专有字段应挂在端点名下。
- 【决策】`api_key_env` 在每次请求时读取，变量缺失时不发送 `authorization` 头也不报错。依据：本地端点（无鉴权）是本 crate 的主要使用场景之一，缺少密钥不应阻止请求。
- 【决策】流式实现复用 `ferrin_provider_util::stream_driver`：`ChatStreamState` 实现 `StreamMachine`，文本与推理部件 ID 固定为 `txt-0`/`reasoning-0`，工具调用增量在 `function.name` 到达前按 `index` 缓冲；`fail_on_early_error` 把首个输出前的 `error` 帧转换为 `ProviderError::ApiCall`（状态码由 `error.rs` 的启发式表推断），其余 `error` 帧作为终止的 `StreamPart::Error` 产出。流在没有 `finish_reason` 时结束产出 `InvalidResponseData` 错误部件而非带 `error` 结束原因的 `Finish`。依据：与第 9 节相同的重试语义；缺少结束原因意味着流被截断，显式错误部件携带原因说明，`Finish{error}` 无法携带。
- 【决策】图像编辑的输入文件与遮罩只接受 `FileData::Bytes`（与 `ferrin-openai` 一致），单文件用 `image` 字段、多文件用 `image[]`。依据：multipart 表单需要字节内容；下载 URL 属于核心层的职责（`secure_url`）。
- 【决策】`ErrorStructure`、`MetadataExtractor`、`transform_request_body`、`convert_usage` 四个扩展点供专用供应商 crate 覆盖错误体形状、附加元数据、改写请求体与用量映射；`error_structure` 作用于全部四个模型，其余三项只作用于 Chat 模型。
- 【事实】测试位于 `tests/suite/*.rs`（`tests/all.rs` 汇总，43 个用例），fixture 位于 `tests/fixtures/<area>/`；`.chunks.txt` 每行一个 `data:` 事件（无 `event:` 字段，以 `data: [DONE]` 结尾），`ferrin_testing::Fixture` 直接回放。fixture 为手工编写（PV-031）。
- 【事实】第 5 节提到的 `cargo xtask record-fixture` 于 2026-09-14 实现（见[工作区布局](../03-engineering/02-workspace-layout.md)第 6 节）；本 crate 的 fixture 尚未用它重新录制（PV-031）。

## 12. 实现记录（2026-09-14，`ferrin-google`）

- 【事实】模块布局：`config`（`GoogleConfig`/`SharedConfig`、模型路径与动作 URL、origin 级端点、WebSocket URL、`x-goog-api-key` 头与无鉴权头）、`api_types`（`generateContent` 响应、`RpcStatus`、数字或字符串计数的反序列化）、`capabilities`（按模型 ID 推断 Gemini 2/2.5/3 能力与思考上限）、`options`（语言模型与部件级选项）、`json_schema`（JSON Schema → OpenAPI 子集）、`json_accumulator`（`partialArgs` → JSON 文本增量）、`convert_prompt`、`prepare_tools`、`request`、`output`、`stream`、`language_model`、`embedding`、`image`、`speech`、`transcription`、`video`、`files`、`batch/{mod, results}`、`realtime`、`tools`、`error`。最大文件 656 行（`cargo xtask check-module-size`）。
- 【决策】供应商 ID 按模型族区分：语言模型 `<name>.generative-ai`、语音 `<name>.speech`、转写 `<name>.transcription`、批处理 `<name>.batch`、实时 `<name>.realtime`，嵌入、图像、视频与文件使用不带后缀的 `<name>`。依据：按模型族区分的 ID 便于在遥测与错误中按前缀区分所用的 API 族。
- 【决策】选项与元数据读取规范键 `google` 与配置的 `name` 键（后者覆盖前者），结果级元数据在两键不同时同时写入。依据：与第 10 节的 `ferrin-anthropic` 相同，自定义名称的供应商实例仍能读到按规范键编写的选项。
- 【决策】图像模型复用语言模型：`GoogleImageModel::prepare_call` 把 `ImageOptions` 转为 `CallOptions`（`responseModalities: ["IMAGE"]`、`imageConfig`、`googleSearch` 工具），`image_result` 把 `GenerateResult` 的文件部件转为图像；批处理的 `BatchRequest::Image` 走同一路径。依据：Gemini 没有独立的图像端点，复用可保证单次调用与批处理的请求体一致。
- 【决策】流式实现复用 `ferrin_provider_util::stream_driver`：`GoogleStreamState` 实现 `StreamMachine`，文本与推理块 ID 为递增整数，函数调用在 `partialArgs` 到达时经 `JsonAccumulator` 产出 `ToolInputDelta`，`willContinue` 的字符串保持打开直到下一段或收尾。Gemini 流没有流内错误帧，因此不使用 `fail_on_early_error`；提示被拦截时以 `content-filter` 结束并保留 `blockReason`。依据：与第 9、11 节相同的驾驶器；Gemini 的错误只以 HTTP 状态返回。
- 【决策】Gemini 3 及以后回放的助手消息中没有任何带 `thoughtSignature` 的函数调用时，全部函数调用写入 `skip_thought_signature_validator` 哨兵并产生一条 `other` 警告；存在签名调用时不做处理。依据：Google 文档记录的哨兵可避免 HTTP 400；只在整条消息都缺少签名时才判定为应用层丢失了签名。
- 【决策】文件：`filename` 在没有 `displayName` 选项时作为 `displayName` 发送（不警告）；支持上传、元数据与删除，不支持下载；上传后按 `pollIntervalMs`/`pollTimeoutMs` 轮询到 `ACTIVE`。依据：Files API 只有 `displayName` 一个可读名称；下载由核心层的 `secure_url` 处理。
- 【决策】批处理：显示名 `ferrin-batch-<id>`；内联请求体 ≥ 20 MB 时上传 JSONL 文件（`inputConfig.fileName`），状态元数据只在此路径携带 `inputFileId`/`inputFileExpiresAt`；所有请求须使用同一模型；图像请求的 `mask` 与 `n > 1` 返回 `InvalidArgument`。依据：模型是批处理端点的一部分；与 `GoogleImageModel::prepare_call` 的错误类型一致。JSONL 上传路径没有 fixture 覆盖（见 `docs/providers/google.md`）。
- 【决策】视频模型只提供 `do_start`/`do_status`，`do_generate` 返回 `UnsupportedFunctionality`；实时临时令牌请求把 API 密钥作为查询参数 `key` 发送而不带 `x-goog-api-key` 头。依据：Veo 只有 `predictLongRunning`；令牌端点按其接受的鉴权形式（查询参数 `key`）调用。
- 【决策】不在范围内：Interactions API 中转写以外的功能、Live API 流式转写、语音翻译、`downloadToolResultFiles`。依据：见 `docs/providers/google.md`。
- 【事实】测试位于 `tests/suite/*.rs`（`tests/all.rs` 汇总，78 个用例），fixture 位于 `tests/fixtures/<area>/`；`.chunks.txt` 每行一个 `data:` 事件（`ferrin_testing::encode_events_file` 格式，事件内的 JSON 转义需双写反斜杠）。fixture 为手工编写（PV-031）。
