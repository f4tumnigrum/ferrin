# 核心行为清单

本表按能力域列出 Ferrin 核心行为的清单，供实现与评审时对照。每一行对应架构文档中的一条【事实】或【决策】，“Ferrin”列给出对应的类型、函数、配置项或取舍，“说明”列记录范围与原因；详细规则以各节引用的章节为准。

## 1. Prompt 与消息

章节：[Prompt 转换](../01-architecture/05-prompt-conversion.md)、[核心数据模型](../01-architecture/03-core-data-model.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| `prompt` 与 `messages` 互斥 | `Error::InvalidPrompt` | |
| `system` 置于消息最前，默认禁止消息中的 system | `allow_system_in_messages()` 显式放行 | |
| 模型不支持的 URL 由核心下载并内联 | `DefaultDownloader` | 受安全 URL 策略约束；并发上限可配置 |
| `image` 部件归一为 `file`，媒体类型按魔数探测 | `detect_media_type` | 自维护签名表 |
| 审批响应在发送模型前剥离 | 提示词转换阶段 | 审批响应只用于核心层重放 |
| 工具输出规范化（text/json/error） | `create_tool_model_output` | |
| 供应商文件引用透传，由适配器解析 | `FileData` 四态 | 缺失键为 `NoSuchProviderReference` |
| 消息裁剪（推理、工具调用、空消息） | `ferrin_message::prune` | |

## 2. 生成循环

章节：[生成循环与流式管线](../01-architecture/07-generation-loop-and-streaming.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 继续条件（全部客户端调用有输出/拒绝 ∧ 有客户端调用或延迟结果 ∧ 停止条件未满足） | `should_continue` | |
| 默认停止条件为 1 步；Agent 默认 20 步 | `step_count(1)` / `step_count(20)` | |
| 每步前可覆盖模型、工具选择、活动工具、消息、上下文 | `prepare_step` | |
| 工具执行仅在完成原因为 `stop`/`tool-calls` 时进行 | 循环判定 | |
| 供应商执行工具的延迟结果跟踪 | 步骤状态 | |
| 结构化输出解析条件（`stop`，或非 `tool-calls` 且有文本） | `Output` | 不满足时返回 `NoOutputGenerated`；`output` 为泛型而非可选字段 |
| 重试默认 2 次、2 s 起、倍率 2、尊重 `retry-after` 0–60 s | `RetryPolicy` | 可选抖动 |
| 超时配置（total/step/firstChunk/chunk/tool/per-tool） | `Timeouts`，`Duration` 类型 | |
| 请求体、请求消息、响应体、原始分片的记录默认关闭 | `Include` | |
| 响应消息组装规则 | `to_response_messages` | |
| 步骤性能指标 | `StepPerformance` | |
| 独立的对象生成入口 | 不提供 | 单一路径 `generate_text(...).output(...)` |

## 3. 工具

章节：[工具系统](../01-architecture/06-tool-system.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 四种工具种类 | `ToolKind` | |
| 执行函数可返回单值或带初步结果的流 | 统一为 `ToolOutput` 流 | |
| 解析失败 → 修复 → 无效标记而非抛出 | `ParsedToolCall::invalid` | |
| `tool_choice` 违规为无效调用 | 同上 | |
| 输入精炼 | `refine_tool_input` | |
| 审批四态与优先级 | `ApprovalPolicy`、`ApprovalStatus` | |
| HMAC-SHA256 审批签名 | 域字符串 `ferrin-tool-approval-v1` | 常量时间比较 |
| 审批重放时重新校验输入与策略 | `validate_tool_approvals` | |
| 工具指纹与漂移 | `fingerprint_tools`、`detect_tool_drift` | |
| 调用方限制 | `ToolCallers` | |
| 活动工具、工具顺序 | `active_tools`、`tool_order` | |
| 工具上下文 schema | JSON 值 + 校验 | |
| 沙箱会话接口 | `Sandbox` trait | 仅本地测试实现 |
| 工具结果类型化 | 定义时类型化，结果为 JSON + 提取辅助 | [ADR 0012](../04-decisions/2026-09-13-0012-tool-typing-strategy.md) |

## 4. 流式

章节：[生成循环与流式管线](../01-architecture/07-generation-loop-and-streaming.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 管线阶段（工具执行、拼接、弹性、停止门、变换、输出、事件处理） | 流式管线 | |
| 流事件集合 | `StreamEvent`，可序列化 | |
| 多消费视图 | 单事件流 + `Completion` | [ADR 0005](../04-decisions/2026-09-13-0005-stream-result-delivery.md) |
| 流式入口 | 首步请求建立后返回 `Result` | 配置错误在调用点以 `?` 处理 |
| 流级重试与错误回调 | `stream_retries`、`on_error` | 默认禁用 |
| 部件 ID 重映射 | 多步骤流 | |
| 平滑输出切分（word/line/regex/segmenter/detector） | `smooth_stream` | segmenter 基于 `unicode-segmentation` |
| 前端消息流协议 | 不提供 | 事件自身可序列化 |

## 5. 规范层与适配器

章节：[Provider 规范](../01-architecture/04-provider-spec.md)、[供应商实现指南](../01-architecture/17-provider-implementation-guide.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 规范版本 | crate 版本 | 不设运行时多版本共存，[ADR 0011](../04-decisions/2026-09-13-0011-spec-versioning-by-crate-version.md) |
| 12 类接口方法集 | `ferrin-spec` trait | 可选方法 → 默认实现 + 能力查询 |
| 不支持的选项产生警告而非错误 | `Warning::Unsupported` | 适配器契约第 1 条 |
| 推理等级映射（effort/budget） | `map_reasoning_to_effort`、`map_reasoning_to_budget` | |
| 惰性凭据加载 | 请求构造阶段读取 | 工厂调用不产生 I/O |
| 供应商工具工厂命名空间 | `openai::tools` 等 | |
| 工作流序列化钩子 | 不提供 | |

## 6. 中间件、注册表、遥测

章节：[中间件与注册表](../01-architecture/10-middleware-and-registry.md)、[可观测性](../01-architecture/13-observability.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 中间件六个钩子、反序包裹 | `LanguageModelMiddleware`、`wrap_language_model` | |
| 内置五种中间件 | `middleware` 模块 | |
| 注册表 `provider:model` 解析与错误 | `ProviderRegistry` | |
| 自定义供应商与回退 | `custom_provider` | |
| 全局默认供应商 | 不提供 | [ADR 0008](../04-decisions/2026-09-13-0008-no-implicit-default-provider.md) |
| 遥测回调接口与选项 | `Telemetry`、`TelemetryOptions` | 回调为同步方法 |
| 全局遥测注册表与诊断通道 | 不提供 | 以 `tracing` 替代 |
| 警告日志全局开关 | 不提供 | `tracing` target 过滤 |

## 7. HTTP 与安全

章节：[HTTP 传输与安全](../01-architecture/14-http-and-security.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 可注入的 HTTP 实现 | `HttpTransport` | |
| 响应处理器族 | `handlers` 模块 | |
| 可重试状态码 408/409/429/5xx | `ApiCallError::is_retryable` | |
| 安全 URL 规则 | `secure_url` | Clippy `disallowed-methods` 强制 |
| JSON 原型污染防护 | 不需要 | Rust 无此风险；改为资源限制 |
| ID 生成器（前缀 + 随机） | `IdGenerator` | |
| User-Agent 后缀链 | `ferrin/<version>` → `ferrin-<provider>/<version>` | |

## 8. 其他模态

章节：[其他模态](../01-architecture/11-other-modalities.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 多值嵌入分块与并发 | `embed_many` | UTF-8 字节数度量（PV-011） |
| 图像生成多次调用、空结果重试 | `generate_image` | |
| 语音、转写、重排 | `generate_speech`、`transcribe`、`rerank` | |
| 视频生成轮询/Webhook/回退 | `generate_video` | Webhook 工厂由应用实现 |
| 文件与技能上传 | `upload_file`、`upload_skill` | |
| 批处理五个函数 | `batch` 模块 | |
| 实时会话 | `realtime`（feature） | 无浏览器传输 |
| 语音翻译（流式） | `SpeechTranslationModel` | |

## 9. MCP

章节：[MCP 集成](../01-architecture/15-mcp.md)。

| 行为 | Ferrin | 说明 |
| --- | --- | --- |
| 传输配置与自定义传输接口 | `TransportConfig`、`McpTransport` | 回调 → 事件流 |
| 协议版本探测与协商 | 双代客户端 | |
| 工具桥接（动态/类型化） | `client.tools(ToolsOptions)` | 服务器 schema 的工具以 `strict = false` 交给供应商 |
| 工具调用重试判定 | `McpError::is_retryable_tool_call` | |
| 资源、提示、补全、诱导、MCP Apps、请求头绑定、OAuth | 对应方法与 feature | |
| stdio 传输 | feature `stdio` | Rust 应用常见场景 |

## 10. 工程实践

工程实践（工作区布局、编码规范、测试、CI、版本与发布、ADR 流程、安全实践、文档规范）见 [`docs/03-engineering/`](../03-engineering/)，不在本表重复。
