# Anthropic（`ferrin-anthropic`）

[English](../../providers/anthropic.md) | **简体中文**

`ferrin-anthropic` 是 Anthropic 的供应商适配器 crate（L3，见[crate 划分](../01-architecture/02-crates.md)）。本文记录 2026-09-13 实现的能力、设置、供应商选项与元数据、限制以及 fixture 清单；实现结构见[Provider 适配器实现指南第 10 节](../01-architecture/17-provider-implementation-guide.md#10-实现记录2026-09-13ferrin-anthropic)。

供应商 ID 形如 `<name>.<family>`，`name` 默认 `anthropic`（`AnthropicSettings::name` 可覆盖）；`provider_options` 与 `provider_metadata` 始终读取/写入 `anthropic` 键，`name` 与之不同时同时读取 `name` 键（后者覆盖前者）并把结果级元数据复制到 `name` 键下。

## 能力矩阵

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| 语言模型：Messages（生成/流式） | 已实现 | `AnthropicMessagesLanguageModel`，`POST /messages`；`AnthropicProvider::messages`（别名 `chat`）与 `Provider::language_model` 返回此族。源码 `src/messages.rs`、`src/request/`、`src/output/`、`src/stream.rs`。 |
| 工具调用 / 供应商工具 | 已实现 | 函数工具（`strict`、`cacheControl`、`deferLoading`、`allowedCallers`、`eagerInputStreaming`、`input_examples`）、`ToolChoice` 全部变体；供应商工具 `anthropic.bash_*`、`anthropic.computer_*`、`anthropic.text_editor_*`、`anthropic.memory_20250818`、`anthropic.web_search_*`、`anthropic.web_fetch_*`、`anthropic.code_execution_*`、`anthropic.tool_search_regex_20251119`、`anthropic.tool_search_bm25_20251119`、`anthropic.advisor_20260301`（工厂 `AnthropicTools`，源码 `src/tools.rs`；ID 与线格式的对应表见 `src/prepare_tools.rs`）。 |
| 结构化输出 | 已实现 | 支持 `output_config.format` 的模型（`structuredOutputMode: auto`/`outputFormat`）写入 `{type: json_schema, schema}`，schema 经 `json_schema::sanitize_json_schema` 收紧；其余模型或 `structuredOutputMode: jsonTool` 回退为名为 `json` 的函数工具（`tool_choice: {type: any, disable_parallel_tool_use: true}`），响应中的 `json` 工具调用映射为文本，`tool_use` 停止原因映射为 `stop`。 |
| 推理 | 已实现 | `ReasoningEffort` 映射为扩展思考：支持自适应思考的模型写入 `thinking: {type: adaptive, display: summarized}` 与 `output_config.effort`（`xhigh` 在不支持时降为 `max` 并警告），其余模型按 `map_reasoning_to_budget` 写入 `thinking: {type: enabled, budget_tokens}`；`ReasoningEffort::None` 写入 `{type: disabled}`。响应的 `thinking`/`redacted_thinking` 块映射为推理部件。 |
| 引用 | 已实现 | 文档 `citations.enabled`、Web 搜索与 Web 抓取的引用映射为 `Source::Document`/`Source::Url`；文本部件保留原始 Web 引用于元数据。 |
| 嵌入 / 图像 / 语音 / 转写 / 重排 / 视频 / 实时 | 无 | Anthropic 无对应 API；`Provider::embedding_model`、`image_model` 返回 `NoSuchModelError`，其余模态沿用 `Provider` 的默认实现。 |
| 文件 | 部分 | `AnthropicFiles::upload_file`（multipart `POST /files`，beta `files-api-2025-04-14`）；元数据、下载、删除未实现（`supports_*` 返回 `false`）。 |
| 技能 | 已实现 | `AnthropicSkills::upload_skill`（multipart `POST /skills`，`files[]`，beta `skills-2025-10-02`）；响应含 `latest_version` 时追加 `GET /skills/{id}/versions/{version}` 读取名称与描述。 |
| 批处理 | 已实现 | `AnthropicBatch`：`POST /messages/batches`（每个请求为一个 Messages 请求体）、状态、结果（`results_url` 的 JSONL 流）、取消、列表（`limit`、`after_id`）。只接受 `BatchRequest::Text`。 |

## 设置与环境变量

【事实】`create_anthropic(AnthropicSettings)`（`src/lib.rs`）：

| 设置 | 环境变量 | 行为 |
| --- | --- | --- |
| `base_url` | `ANTHROPIC_BASE_URL` | 默认 `https://api.anthropic.com/v1`；只有 origin 的 URL 追加 `/v1`，尾部斜杠被去除；无效 URL 时 `create_anthropic` 立即失败。 |
| `api_key` | `ANTHROPIC_API_KEY` | 写入 `x-api-key`；首次请求时读取，缺失时该请求以 `ProviderError::LoadApiKey` 失败。 |
| `auth_token` | `ANTHROPIC_AUTH_TOKEN` | 写入 `authorization: Bearer <token>`；与 `api_key` 同时设置返回 `InvalidArgument`；环境变量只在 `ANTHROPIC_API_KEY` 缺失时生效。 |
| `headers` | 无 | 附加到每个请求；调用级 `headers` 覆盖同名头；两处的 `anthropic-beta` 与请求推导出的 beta 合并。 |
| `name` | 无 | 供应商 ID 前缀、附加选项键与文件/技能引用键（默认 `anthropic`）。 |
| `transport` / `id_generator` | 无 | 默认共享的 `reqwest` 传输与随机 ID 生成器（来源 ID）。 |

【事实】每个请求携带 `anthropic-version: 2023-06-01`，`user-agent` 追加 `ferrin-anthropic/<crate 版本>`（`config::USER_AGENT`）；`anthropic-beta` 为去重、小写、排序后以逗号连接的 beta 列表，为空时不发送。

【事实】`AnthropicConfig` 另有两项供兼容端点调整的字段：`supports_strict_tools`（默认 `true`，为 `false` 时忽略函数工具的 `strict` 并警告）与 `supports_native_structured_output`（默认 `true`，为 `false` 时结构化输出一律回退为 `json` 工具）。

## 供应商选项（`provider_options["anthropic"]`）

【决策】选项键为 camelCase，未知键按参考 Zod 对象解析规则忽略，已知字段的非法值和非法枚举值返回 `ProviderError::InvalidArgument`。完整 schema 见 `src/options.rs`。来源：本地 AI SDK `6c6c221` 的 `anthropic-language-model-options.ts`。

【事实】语言模型：`sendReasoning`、`structuredOutputMode`（`outputFormat`/`jsonTool`/`auto`）、`thinking {type: adaptive | enabled | disabled, budgetTokens, display: omitted | summarized | updates, blockBinding {prefixMismatchBehavior: error | drop_block}}`、`disableParallelToolUse`、`cacheControl {type: ephemeral, ttl: 5m | 1h}`、`metadata {userId}`、`mcpServers [{type: url, name, url, authorizationToken, toolConfiguration {enabled, allowedTools}}]`、`container {id, skills [{type: anthropic, skillId, version} | {type: custom, providerReference, version}]}`、`toolStreaming`、`effort`（`low`/`medium`/`high`/`xhigh`/`max`）、`taskBudget {type: tokens, total ≥ 20000, remaining}`、`speed`（`fast`/`standard`）、`serviceTier`（`auto`/`standard_only`）、`inferenceGeo`（`us`/`global`）、`fallbacks`（`"default"` 或模型对象数组）、`anthropicBeta [..]`、`contextManagement {edits: [clear_tool_uses_20250919 | clear_thinking_20251015 | compact_20260112]}`。

【事实】部件与消息级选项：文件部件 `containerUpload`、`citations {enabled}`、`title`、`context`；系统消息 `clearAt`、`effort`、`toolChanges [{type, toolName}]`（非首条系统消息内联发送，各自追加 `mid-conversation-*` beta）；推理部件 `signature`/`redactedData`；工具调用部件 `caller`、`type: mcp-tool-use` 与 `serverName`；消息与部件级 `cacheControl`（最多 4 个断点，超出或落在不支持的位置时警告并忽略）。函数工具选项：`cacheControl`、`deferLoading`、`allowedCallers`、`eagerInputStreaming`。批处理：批次级 `anthropicBeta`。

【事实】选项触发的 beta：`mcpServers`→`mcp-client-2025-04-04`；`container`→`code-execution-2025-08-25`、`skills-2025-10-02`、`files-api-2025-04-14`；`contextManagement`→`context-management-2025-06-27`（含 `compact_20260112` 时另加 `compact-2026-01-12`）；`taskBudget`→`task-budgets-2026-03-13`；`speed: fast`→`fast-mode-2026-02-01`；`thinking.display: updates`→`thinking-display-updates-2026-08-18`；`thinking.blockBinding`→`thinking-binding-controls-2026-08-01`；`fallbacks: "default"`→`server-side-fallback-2026-07-01`，模型数组→`server-side-fallback-2026-06-01`；PDF 文档→`pdfs-2024-09-25`；文件引用→`files-api-2025-04-14`；带 `strict` 或位于支持结构化输出模型上的函数工具→`structured-outputs-2025-11-13`；`allowedCallers`/`input_examples`→`advanced-tool-use-2025-11-20`；供应商工具按类型追加各自的 beta（`src/prepare_tools.rs`）。

## 供应商元数据（`provider_metadata["anthropic"]`）

【事实】结果级：`usage`（原始用量对象）、`stopSequence`、`stopDetails`、`inputTransformations`、`iterations`（无则 `null`）、`container {id, expiresAt, skills}`（无则 `null`）、`contextManagement {appliedEdits}`（无则 `null`）；`name` 非 `anthropic` 时同一对象复制到 `name` 键。

【事实】部件级：文本部件 `citations`（Web 引用原文）；推理部件 `signature` 或 `redactedData`；工具调用部件 `caller {type, toolId}`，MCP 调用 `{type: mcp-tool-use, serverName}`；来源 `citedText`、`encryptedIndex`（Web 引用）、`startPageNumber`/`endPageNumber` 或 `startCharIndex`/`endCharIndex`（文档引用）、`pageAge`（Web 搜索结果）；压缩块映射为带 `{type: compaction}` 的文本部件；`container_upload` 块映射为 `anthropic.container_upload` 自定义部件。

【事实】用量：`input.total = input_tokens + cache_creation_input_tokens + cache_read_input_tokens`，`input.no_cache = input_tokens`，`output.reasoning` 取自 `output_tokens_details.thinking_tokens`；响应含 `iterations` 时按轮次汇总（压缩轮次计入，advisor 轮次不计入，fallback 轮次替代原轮次）。批处理状态 `requestCounts`、`archivedAt`、`cancelInitiatedAt`、`endedAt`、`resultsUrl`；失败项 `requestId`。文件 `filename`、`mimeType`、`sizeBytes`、`createdAt`、`downloadable`；技能 `source`、`createdAt`、`updatedAt`。

【决策】已有工具工厂校验完整的参考输入/输出 schema，包括动作变体、必需字段、元组长度和默认值。对象输入遵循参考解析器：未知字段被移除，显式字典/透传策略保留的字段除外；严格对象拒绝未知字段。供应商配置参数在请求转换前校验。来源：本地 `6c6c221` 的 `packages/anthropic/src/tool` schema、[ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md)，2026-09-17。

【决策】`UploadData::Stream` 经共享传输进行流式 multipart 上传，不在 HTTP 前把文件收集进内存。取消或丢弃请求释放输入流；源流失败使用脱敏的请求体错误。来源：`src/files.rs` 及参考文件上传实现；[ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md)，2026-09-17。

## 已知限制与警告

- 【事实】`frequencyPenalty`、`presencePenalty`、`seed` 产生 `unsupported` 警告并被丢弃；`temperature` 限制在 `[0, 1]`（越界时钳制并警告）；`temperature` 与 `topP` 同时设置时丢弃 `topP`；启用扩展思考或模型不接受采样参数时丢弃 `temperature`、`topK`、`topP` 并警告。
- 【事实】`maxOutputTokens` 缺省为模型上限（`src/capabilities.rs`：Sonnet 4.x/Haiku 4.5 为 64000，Opus 4.x 为 32000，Sonnet 4.6/Opus 4.6 及以后为 128000，Claude 3 Haiku 与旧代为 4096）；启用预算式思考时 `max_tokens = maxOutputTokens + budget_tokens`；超过上限时截断并警告；未知的 `claude-*` ID 按最新能力处理并警告，非 Claude ID 按保守默认值（4096、无结构化输出）处理。
- 【事实】文件部件：图像接受字节、URL 与文件引用；`application/pdf` 与 `text/plain` 接受字节、URL、文本与文件引用；其他媒体类型返回 `UnsupportedFunctionality`；引用键非 `name` 时返回 `NoSuchProviderReference`；`containerUpload: true` 只对文件引用生效。助手消息中的文件、推理文件与自定义部件产生警告并被忽略。
- 【事实】`sendReasoning: false` 时提示中的推理部件被丢弃并警告；缺少 `signature`/`redactedData` 的推理部件同样被丢弃并警告；最后一条助手消息的文本去除尾部空白。
- 【事实】`ToolResultOutput::ExecutionDenied` 以 `is_error: true` 与文本 `Tool call execution denied.` 发送；供应商执行的调用与结果按工具类型映射为 `server_tool_use`/`mcp_tool_use` 与对应的 `*_tool_result` 块，未知的供应商执行工具产生警告。
- 【事实】`web_search_20260209`/`web_fetch_20260209` 与代码执行工具同用时，工具调用标记为动态；`container.skills` 缺少代码执行工具时警告 `code execution tool is required when using skills`。
- 【决策】流式调用中 `StreamPart::Error` 是终止部件：错误之前未关闭的部件先收到 end 部件，错误之后不再产出 `Finish`。依据：见实现指南第 9 节。
- 【决策】服务器在产出任何输出之前返回的 `error` 事件使 `do_stream` 以 `ProviderError::ApiCall` 失败，状态码由错误类型推断：`api_error`→500、`overloaded_error`→529、`rate_limit_error`→429（以上可重试）、`request_too_large`→413、`authentication_error`→401、`permission_error`→403、`not_found_error`→404、`billing_error`/`invalid_request_error`→400，其余 500；HTTP 错误响应保留服务器状态码。依据：见实现指南第 10 节。
- 【事实】批处理：请求 ID 须匹配 `^[A-Za-z0-9_-]{1,64}$` 且互不重复，否则 `InvalidArgument`；`BatchRequest::Image`、请求级 `anthropicBeta`、`speed`、`fallbacks` 中的 `speed`、别名后的供应商工具名与 `json` 工具回退返回 `UnsupportedFunctionality`；`webhookUrl` 产生警告；进行中或已归档的批次读取结果返回 `InvalidArgument`，缺少 `results_url` 返回 `InvalidResponseData`；`results_url` 与 `base_url` 不同源时不携带凭据。
- 【事实】同一流中收到不同 `message_start` ID 时产出 `InvalidResponseData` 错误部件并结束流。

## Fixture 清单

fixture 位于 `crates/providers/ferrin-anthropic/tests/fixtures/<area>/`，由 `tests/suite/*.rs` 通过 `ferrin_testing::FixtureServer` 回放；流式用例以 `-stream` 后缀命名，`.chunks.txt` 每行一个 `event:`/`data:` 事件。

| 区域 | 用例 | 覆盖 |
| --- | --- | --- |
| `messages` | `text-basic`、`tool-call`、`reasoning`、`web-search`、`citations`、`json-tool`、`error-400`、`error-429`、`text-basic-stream`、`tool-call-stream`、`reasoning-stream`、`code-execution-stream`、`json-tool-stream`、`error-early-stream`、`error-late-stream` | 文本与缓存用量、工具调用、思考与签名、Web 搜索结果与引用来源、文档引用、`json` 工具回退、错误映射、流式契约、代码执行与容器元数据、早期与晚期错误事件 |
| `files` | `upload` | multipart 上传、beta 头、元数据 |
| `skills` | `upload`、`upload-no-version`、`version` | multipart `files[]`、版本读取、元数据 |
| `batch` | `create`、`status-in-progress`、`cancel`、`list`、`results.jsonl` | 请求体、状态与计数映射、结果流（成功/错误/取消/过期/未知）、取消与分页 |

请求体、提示转换、工具线格式与流式部件序列以 `insta` 快照记录于 `tests/suite/snapshots/`；测试共 58 个。

【待验证】（PV-031）以上 fixture 依据供应商公开 API 文档的响应 schema 手工编写；`record-fixture` 实现后需用真实响应重新录制。

【事实】 `thinking.blockBinding.prefixMismatchBehavior` 使用 camelCase 选项名称，在线格式中转换为 `thinking.block_binding.prefix_mismatch_behavior`；同时兼容原有 snake_case 选项。回归：`tests/suite/messages_request.rs::documented_block_binding_options_use_camel_case`（2026-09-15）。

【事实】 结构化输出清理保留 `$ref` 的同级 `$id`、`$defs` 与 `definitions`，清理定义但不展开递归引用。回归：`tests/suite/unit.rs::schema_references_retain_definitions_and_scope`（2026-09-15）。

【事实】 供应商定义工具的别名在强制选择中转换为供应商名称，非流式、流式与预填充工具调用均恢复注册时的名称。回归：`tests/suite/tools.rs::provider_tool_aliases_roundtrip_through_choices_and_calls`（2026-09-15）。

【事实】 每个请求的所有提示部件与工具定义共享四个缓存断点的上限，超出部分被移除并产生警告。回归：`tests/suite/messages_request.rs::cache_breakpoint_limit_is_shared_across_prompt_and_tools`（2026-09-15）。

【事实】 批次结果 URL 经过 `url_policy` 校验：默认只允许 HTTPS 与公网地址，固定解析后的地址，拒绝重定向，流式响应字节数受 `max_body_bytes` 限制。凭据与调用方头仅发送到配置的同源地址或明确的 `credentialed_origins`。本地测试端点需要显式 `allow_http().trust_origin(...)`。来源：`src/batch/`、`tests/suite/security.rs`（2026-09-15）。

【决策】`code_execution_20250825` 与 `code_execution_20260120` 绑定供应商调用者并支持延迟结果，追加自身类型时保留已有 `allowedCallers`。来源：[ADR 0022](../04-decisions/2026-09-17-0022-provider-tool-roundtrips.md)；调用者准备回归，实时 API 验证仍属于 PV-031。

【事实】新版代码执行工厂校验程序化、bash 和文本编辑输入及相应结果变体；20260120 工厂还接受加密执行输出。来源：`src/tools/code_execution.rs` 与 `tests/suite/tools.rs`（2026-09-17）；仅进行确定性 Schema/调用者测试。
