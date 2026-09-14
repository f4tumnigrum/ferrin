# Crate 划分与职责

## 1. 命名规则

【决策】所有 crate 以 `ferrin-` 为前缀，库名（`[lib] name`）使用下划线形式（`ferrin_spec`）。依据：统一前缀便于在依赖树中识别第一方 crate，也避免与 crates.io 上的通用名冲突。

## 2. Crate 清单

| Crate | 层 | 职责 | 允许依赖的第一方 crate |
| --- | --- | --- | --- |
| `ferrin-spec` | L0 | Provider 规范：模型 trait、Prompt 与内容部件、调用选项、流事件、用量、完成原因、警告、供应商选项/元数据/引用、规范层错误、对象安全适配。 | 无 |
| `ferrin-schema` | L1 | `Schema<T>` 抽象、JSON Schema 生成设置、校验、部分 JSON 修复、JSON 解析限制。 | `ferrin-spec`（错误类型） |
| `ferrin-message` | L1 | 应用侧消息模型：`Message`、内容部件便捷形式、`FileSource`、`ToolResultOutput`、审批响应。 | `ferrin-spec` |
| `ferrin-provider-util` | L2 | 供应商适配器公共工具：HTTP 传输抽象、响应处理器、SSE 解码、重试分类、设置加载、供应商选项解析、ID 生成、媒体类型探测、User-Agent、安全 URL 与下载、推理等级映射、工具名映射、流状态机驱动（`stream_driver`）。 | `ferrin-spec` |
| `ferrin-tool` | L2 | 工具定义（`Tool`、`ToolKind`、`ToolSet`）、执行上下文、输出规范化、审批声明、调用方限制、`Sandbox` trait。 | `ferrin-spec`、`ferrin-schema`、`ferrin-message` |
| `ferrin-openai` | L3 | OpenAI 适配器：Responses、Chat Completions、Completions、Embeddings、Images、Speech、Transcription、Speech Translation、Files、Skills、Batch、Realtime。 | `ferrin-spec`、`ferrin-schema`、`ferrin-provider-util` |
| `ferrin-anthropic` | L3 | Anthropic 适配器：Messages、Files、Skills、Batch、供应商工具。 | 同上 |
| `ferrin-openai-compatible` | L3 | OpenAI 兼容端点通用适配器（Chat、Completion、Embedding、Image），供第三方端点直接使用或被其他适配器复用。 | 同上 |
| `ferrin-google` | L3 | Google Generative AI 适配器。 | 同上 |
| `ferrin-mcp` | L3 | MCP 客户端：传输（Streamable HTTP、SSE、stdio）、OAuth、工具桥接、资源与提示、诱导（elicitation）。 | `ferrin-spec`、`ferrin-schema`、`ferrin-tool`、`ferrin-provider-util`（2026-09-14 实现时未用到 `ferrin-message`，见 [MCP 集成](15-mcp.md)第 6 节） |
| `ferrin-core` | L4 | 核心：Prompt 标准化与转换、文本生成循环、流式管线、结构化输出、Agent、中间件、注册表、重试与超时、遥测接口、其他模态函数、核心错误。 | L0–L2 全部 |
| `ferrin-otel` | L5 | `Telemetry` 的 OpenTelemetry 实现，遵循 GenAI 语义约定。 | `ferrin-core`、`ferrin-spec`、`ferrin-tool`（2026-09-14 实现：`opentelemetry_sdk` 仅为测试依赖，见 [可观测性](13-observability.md)第 9 节） |
| `ferrin-testing` | L5 | 测试辅助：Mock 模型、流模拟、fixture 回放、确定性 ID/时钟、HTTP 录制回放。 | `ferrin-core`、`ferrin-provider-util` |
| `ferrin-macros` | L5 | 过程宏：`#[ferrin::tool]`。 | 无（生成代码引用 `::ferrin::tool::*` 路径，因此只能经门面使用；2026-09-14 实现，见[工具系统](06-tool-system.md)第 1.1 节） |
| `ferrin` | L5 | 门面：re-export `ferrin-core` 公共 API 与 `prelude`；通过 feature 启用供应商与扩展 crate。 | 全部 |
| `xtask` | 工程 | 仓库自动化（fixture 录制、版本检查、发布顺序）。不发布。 | 任意 |

【事实】2026-09-13 实现的 `ferrin-provider-util` 只依赖 `ferrin-spec`：`parse_provider_options` 以 serde 反序列化完成校验，不需要 `JsonSchema`（见 [HTTP 传输与安全](14-http-and-security.md)第 7 节），原设计中对 `ferrin-schema` 的依赖取消。

## 3. 依赖图

```
ferrin-spec
  ├── ferrin-schema
  ├── ferrin-message
  │     └── ferrin-tool ──────────────┐
  ├── ferrin-provider-util ───────────┤
  │     ├── ferrin-openai             │
  │     ├── ferrin-anthropic          │
  │     ├── ferrin-openai-compatible  │
  │     ├── ferrin-google             │
  │     └── ferrin-mcp ◄──────────────┘
  └── ferrin-core ◄── (spec, schema, message, tool, provider-util)
        ├── ferrin-otel
        ├── ferrin-testing
        └── ferrin (facade) ◄── providers, mcp, otel (feature-gated)
```

【决策】`ferrin-mcp` 不依赖 `ferrin-core`。依据：MCP 工具以动态工具形态接入工具集，只需要 `ferrin-tool` 与 `ferrin-provider-util`，不需要核心循环的类型；不依赖核心层也让 MCP 客户端可以单独用于非生成场景。

## 4. 各 crate 模块结构

### 4.1 `ferrin-spec`

```
src/
  lib.rs                 // 显式 re-export；SPEC_VERSION
  json.rs                // JsonValue = serde_json::Value, JsonObject 别名与辅助
  shared/
    provider_options.rs  // ProviderOptions, ProviderMetadata
    provider_reference.rs
    warning.rs
    headers.rs           // Headers 类型（http::HeaderMap 封装）
    ids.rs               // ProviderId, ModelId, ToolCallId, ToolName, ApprovalId
  language_model/
    mod.rs               // LanguageModel trait
    call_options.rs      // CallOptions, ResponseFormat, ReasoningEffort, ToolChoice
    prompt.rs            // Prompt, PromptMessage, 部件类型, FileData
    tool.rs              // ToolDefinition::{Function, Provider}
    content.rs           // Content 枚举（生成结果部件）
    stream_part.rs       // StreamPart 枚举
    result.rs            // GenerateResult, StreamResult, ResponseMetadata, RequestMetadata
    finish_reason.rs
    usage.rs
  embedding_model.rs
  image_model.rs
  speech_model.rs
  transcription_model.rs
  reranking_model.rs
  video_model.rs
  files.rs
  skills.rs
  batch.rs
  realtime_model.rs
  speech_translation_model.rs
  provider.rs            // Provider trait
  dynamic/               // Dyn* 对象安全适配 trait 与 blanket impl
  error/                 // 规范层错误类型
```

### 4.2 `ferrin-core`

```
src/
  lib.rs
  error.rs
  ids.rs                 // IdGenerator 与默认实现
  retry.rs
  timeout.rs
  prompt/
    standardize.rs
    convert.rs           // Message → spec::Prompt
    download.rs          // URL 下载策略与 DownloadFn
    prepare_tools.rs
    prepare_tool_choice.rs
    call_options.rs      // 应用侧 CallSettings → spec::CallOptions 校验
  generate_text/
    builder.rs
    run.rs               // 多步循环
    step.rs              // StepResult, StepContent
    parse_tool_call.rs
    repair.rs
    execute_tool.rs
    approval/            // 解析、签名、收集、校验、指纹
    stop_condition.rs
    prepare_step.rs
    response_messages.rs
    result.rs
  stream_text/
    builder.rs
    pipeline/            // 每个阶段一个模块
    events.rs            // StreamEvent
    result.rs            // StreamTextResult, Completion
    transforms/          // smooth_stream 等
  output/                // Output 策略：text, object, array, choice, json
  agent/
    mod.rs               // Agent trait
    tool_loop_agent.rs
  middleware/
    mod.rs               // LanguageModelMiddleware, ImageModelMiddleware
    wrap.rs
    builtin/             // default_settings, extract_reasoning, simulate_streaming, extract_json, add_tool_input_examples
  registry/
    provider_registry.rs
    custom_provider.rs
    default.rs           // 进程级默认注册表（显式设置）
  telemetry/
    mod.rs               // Telemetry trait, TelemetryOptions
    dispatcher.rs
    spans.rs             // tracing span 命名与字段
  embed/
  image/
  speech/
  transcription/
  rerank/
  video/
  files/
  skills/
  batch/
  realtime/
  speech_translation/
```

模块规模约束见[编码规范](../03-engineering/03-coding-standards.md)：单模块目标 500 行以内，超过 800 行必须拆分。

### 4.3 供应商 crate 标准布局

```
ferrin-openai/
  src/
    lib.rs                 // create_openai(), OpenAiProvider, 设置类型
    config.rs              // 内部配置：base_url, headers fn, transport, id generator
    error.rs               // 错误响应 schema 与 failed_response_handler
    responses/             // 一个子目录对应一个 API 族
      language_model.rs
      convert_prompt.rs    // spec::Prompt → API 请求消息
      convert_tools.rs
      map_finish_reason.rs
      options.rs           // provider options 的类型与 schema
      api_types.rs         // 响应/分片 serde 类型
    chat/
    completion/
    embedding/
    image/
    speech/
    transcription/
    files/
    skills/
    batch/
    realtime/
    tools/                 // 供应商定义/执行工具的工厂
  tests/
    fixtures/<api>/<case>.{chunks.txt,json}
    suite/                 // 集成测试（wiremock 回放）
```

## 5. Feature 门控

| Crate | Feature | 作用 | 默认 |
| --- | --- | --- | --- |
| `ferrin-schema` | `json-schema-validation` | 启用 `jsonschema` crate 对无 Rust 类型的动态输入做校验 | 开 |
| `ferrin-provider-util` | `reqwest` | 提供 `ReqwestTransport` 默认实现 | 开 |
| `ferrin-provider-util` | `platform-verifier` | 直接依赖 `rustls-platform-verifier` 以配置系统证书校验（reqwest 0.13 默认已启用该校验器；无 `native-tls` feature，见 CI 文档第 3 节） | 关 |
| `ferrin-tool` | `sandbox` | 编译 `Sandbox` trait 与相关执行上下文字段 | 关 |
| `ferrin-core` | `realtime` | 编译实时会话 API（引入 WebSocket 依赖） | 关 |
| `ferrin-core` | `video` | 编译视频生成 API（轮询/Webhook） | 开 |
| `ferrin-mcp` | `stdio` | 子进程 stdio 传输 | 开 |
| `ferrin-mcp` | `oauth` | OAuth 授权流程 | 开 |
| `ferrin` | `openai`、`anthropic`、`google`、`openai-compatible`、`mcp`、`otel`、`macros`、`realtime` | 启用对应 crate 并在 `ferrin::providers::*` 与 crate 根（`ferrin::openai` 等）下 re-export；`realtime` 转发 `ferrin-core/realtime`（2026-09-14 实现，见 [API 参考](../02-api/02-api-reference.md)第 14 节） | `macros` 开，其余关 |

【决策】工作区内部 crate 之间不使用可选 feature 改变公共类型的形状（feature 只增删 API，不改变已有签名）。依据：Cargo feature 统一（unification）会让下游组合出未测试的形状；完全禁止 feature 又会让外部用户无法裁剪依赖，因此保留少量 feature，但限制其语义为纯增量。

## 6. 发布单元与版本联动

- 每个 crate 独立版本号；`ferrin-spec` 的破坏性变更会级联到所有供应商 crate 与核心 crate。
- 门面 crate `ferrin` 的版本跟随 `ferrin-core`。
- 联动规则与流程见[版本与发布](../03-engineering/06-versioning-and-release.md)。
