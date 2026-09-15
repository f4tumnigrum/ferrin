# OpenAI（`ferrin-openai`）

[English](../../providers/openai.md) | **简体中文**

`ferrin-openai` 是 OpenAI 的供应商适配器 crate（L3，见[crate 划分](../01-architecture/02-crates.md)）。本文记录 2026-09-13 实现的能力、设置、供应商选项与元数据、限制以及 fixture 清单；实现结构见[Provider 适配器实现指南第 9 节](../01-architecture/17-provider-implementation-guide.md#9-实现记录2026-09-13ferrin-openai)。

供应商 ID 形如 `<name>.<family>`，`name` 默认 `openai`（`OpenAiSettings::name` 可覆盖）；`provider_options` 与 `provider_metadata` 默认使用 `openai` 键（`OpenAiConfig::provider_options_key`）。

## 能力矩阵

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| 语言模型：Responses（生成/流式） | 已实现 | `OpenAiResponsesLanguageModel`，`POST /responses`；`Provider::language_model` 返回此族。源码 `src/responses/`。 |
| 语言模型：Chat Completions（生成/流式） | 已实现 | `OpenAiChatLanguageModel`，`POST /chat/completions`。源码 `src/chat/`。 |
| 语言模型：Completions（生成/流式） | 已实现 | `OpenAiCompletionLanguageModel`，`POST /completions`；提示转换为 `user:`/`assistant:` 文本，不支持工具。源码 `src/completion/`。 |
| 工具调用 / 供应商工具 | 已实现 | 函数工具（含 `strict`）、`ToolChoice` 全部变体；供应商工具 `openai.web_search`、`openai.web_search_preview`、`openai.file_search`、`openai.code_interpreter`、`openai.image_generation`、`openai.mcp`、`openai.tool_search`、`openai.programmatic_tool_calling`、`openai.apply_patch`、`openai.local_shell`、`openai.shell`、`openai.computer`、`openai.custom`（工厂 `OpenAiTools`，源码 `src/tools/mod.rs`）。Chat/Completions 只接受函数工具。 |
| 结构化输出 | 已实现 | Responses：`text.format = json_schema`（`strict` 默认 `true`，可用 `strictJsonSchema` 关闭）或 `json_object`；Chat：`response_format.json_schema`。schema 经 `json_schema::normalize_json_schema` 规范化，不支持的关键字产生警告。 |
| 推理 | 已实现 | `ReasoningEffort` 的非默认值写入 `reasoning.effort`（Responses）/`reasoning_effort`（Chat）；推理模型按模型 ID 推断（o 系列、GPT-5 及以后，`chat` 变体除外，`src/capabilities.rs`）；Responses 的 `reasoning.summary` 默认 `detailed`；GPT-6 及以后只接受 `low`/`medium`/`high`/`xhigh`/`max`。 |
| 嵌入 | 已实现 | `OpenAiEmbeddingModel`，`POST /embeddings`，每次最多 2048 个值，`encoding_format: float`。 |
| 图像 | 已实现 | `OpenAiImageModel`，`POST /images/generations`；带 `files`/`mask` 时改用 multipart `POST /images/edits`。`dall-e-3` 每次 1 张，其余 10 张；`dall-e-*` 显式请求 `b64_json`。 |
| 语音合成 | 已实现 | `OpenAiSpeechModel`，`POST /audio/speech`；输出格式 `mp3`、`opus`、`aac`、`flac`、`wav`、`pcm`，默认 `mp3`，默认声音 `alloy`。 |
| 转写 | 已实现 | `OpenAiTranscriptionModel`，multipart `POST /audio/transcriptions`；`gpt-realtime-whisper*` 模型只支持流式（WebSocket，需 feature `realtime`）。 |
| 语音翻译 | 已实现（feature `realtime`） | `OpenAiSpeechTranslationModel`，WebSocket `/realtime/translations?model=<id>`。 |
| 重排 | 无 | OpenAI 无对应 API；`Provider::reranking_model` 返回 `NoSuchModelError`。 |
| 视频 | 未实现 | `Provider::video_model` 返回 `NoSuchModelError`。 |
| 文件 | 已实现 | `OpenAiFiles`：上传（multipart `POST /files`）、元数据（`GET /files/{id}`）、下载（`GET /files/{id}/content`）、删除。 |
| 技能 | 已实现 | `OpenAiSkills::upload_skill`（multipart `POST /skills`，`files[]`）。 |
| 批处理 | 已实现 | `OpenAiBatch`：上传 JSONL 到 `/files`（`purpose: batch`）后 `POST /batches`；状态、结果（输出与错误文件 JSONL 流）、取消、列表。只接受 `BatchRequest::Text`，一个批次内模型 ID 必须一致，端点固定为 `/v1/responses`，窗口 `24h`。 |
| 实时 | 已实现 | `OpenAiRealtimeFactory`/`OpenAiRealtimeModel`：`POST /realtime/client_secrets` 获取临时密钥，会话 URL `wss://…/realtime?model=<id>`，`RealtimeSessionConfig` 与事件的双向映射；连接由 `ferrin-core` 的 `realtime` feature 驱动。 |

## 设置与环境变量

【事实】`create_openai(OpenAiSettings)`（`src/lib.rs`）：

| 设置 | 环境变量 | 行为 |
| --- | --- | --- |
| `base_url` | `OPENAI_BASE_URL` | 默认 `https://api.openai.com/v1`；尾部斜杠被去除；无效 URL 时 `create_openai` 立即失败。 |
| `api_key` | `OPENAI_API_KEY` | 首次请求时读取；缺失时该请求以 `ProviderError::LoadApiKey` 失败。 |
| `organization` / `project` | 无 | 写入 `openai-organization` / `openai-project` 头。 |
| `headers` | 无 | 附加到每个请求；调用级 `headers` 覆盖同名头。 |
| `name` | 无 | 供应商 ID 前缀与文件引用键（默认 `openai`）。 |
| `transport` / `id_generator` | 无 | 默认共享的 `reqwest` 传输与随机 ID 生成器。 |

每个请求的 `user-agent` 追加 `ferrin-openai/<crate 版本>`（`config::USER_AGENT`）。

【事实】`OpenAiConfig` 另有三项供第三方端点调整的字段：`provider_options_key`（默认 `openai`）、`explicit_message_item_type`（默认 `false`，为 Responses 消息项附加 `type: "message"`）、`supports_web_search_sources_include`（默认 `true`）、`file_id_prefixes`（默认 `["file-"]`，识别 `FileData::Text` 中的文件 ID）。

【事实】feature `realtime`（默认关闭）引入 `tokio-tungstenite`，启用 `OpenAiSpeechTranslationModel`、`OpenAiProvider::speech_translation` 与 `gpt-realtime-whisper*` 的流式转写。未启用时 `Provider::speech_translation_model` 返回提示启用该 feature 的 `NoSuchModelError`，`TranscriptionModel::supports_stream` 返回 `false`，`do_stream` 返回 `UnsupportedFunctionality`。

【事实】WebSocket 连接以子协议 `realtime` 与 `openai-insecure-api-key.<key>` 认证，请求头中不携带 `authorization`；URL 由 `OpenAiConfig::websocket_url` 从 `base_url` 派生（`https`→`wss`，`http`→`ws`）。

## 供应商选项（`provider_options["openai"]`）

选项键为 camelCase，未知键返回 `ProviderError::InvalidArgument`。完整 schema 见各模块的 `*ProviderOptions` 结构体。

【事实】Responses（`src/responses/options.rs`）：`conversation`、`include`、`includeWebSearchSources`、`instructions`、`logprobs`（`true` 或 top-N 数值）、`maxToolCalls`、`metadata`、`parallelToolCalls`、`previousResponseId`、`promptCacheKey`、`promptCacheOptions {retention}`、`promptCacheRetention`、`reasoningEffort`、`reasoningEffortUpdate`、`reasoningSummary`、`reasoningMode`、`reasoningContext`、`safetyIdentifier`、`serviceTier`、`store`、`strictJsonSchema`、`systemMessageMode`（`system`/`developer`/`remove`）、`textVerbosity`、`truncation`、`user`、`forceReasoning`、`contextManagement [{type, compactThreshold}]`、`compactionTrigger`、`passThroughUnsupportedFiles`。部件级选项：`itemId`、`reasoningEncryptedContent`、`phase`、`imageDetail`、`encryptedContent`（`openai.compaction` 自定义部件）。函数工具选项：`deferLoading`、`allowedCallers`、`outputSchema`、`namespace`、`namespaceDescription`。

【事实】Chat（`src/chat/options.rs`）：`logitBias`、`logprobs`、`user`、`parallelToolCalls`、`maxCompletionTokens`、`store`、`metadata`、`prediction`、`reasoningEffort`、`serviceTier`、`promptCacheKey`、`promptCacheOptions`、`promptCacheRetention`、`safetyIdentifier`、`textVerbosity`、`strictJsonSchema`、`systemMessageMode`、`forceReasoning`。

【事实】Completions：`echo`、`logitBias`、`logprobs`、`suffix`、`user`。嵌入：`dimensions`、`user`。图像：`quality`、`style`、`background`、`moderation`、`outputFormat`、`outputCompression`、`inputFidelity`、`user`。语音：`instructions`、`speed`。转写：`include`、`language`、`prompt`、`temperature`、`timestampGranularities`、`responseFormat`、`chunkingStrategy`（`auto` 或 `{type: server_vad, threshold, prefixPaddingMs, silenceDurationMs}`）、`streaming {delay, include}`。文件：`purpose`、`expiresAfter {anchor, seconds}`。批处理：`inputFileExpiresAfter`（秒）。

## 供应商元数据（`provider_metadata["openai"]`）

【事实】Responses：结果级 `responseId`、`serviceTier`、`reasoningContext`、`logprobs`；文本与推理部件 `itemId`、`reasoningEncryptedContent`、`phase`、`annotations`；工具调用部件 `itemId`、`async`、`caller`、`namespace`；供应商工具结果按工具类型携带 `type`、`queries`/`query`、`sources`、`fileId`、`containerId`、`output`、`error`、`index`（`src/responses/output.rs`、`src/responses/stream/items.rs`）。

【事实】Chat：`acceptedPredictionTokens`、`rejectedPredictionTokens`、`logprobs`。Completions：`logprobs`。

【事实】图像：`images[{revisedPrompt}]`、`background`、`outputFormat`、`quality`、`size`、`created`，用量补充 `imageTokens`、`textTokens`。转写（`gpt-4o-transcribe-diarize`）：`segments[{text, startSecond, endSecond, speaker}]`。文件：`purpose`、`status`、`bytes`、`filename`、`createdAt`、`expiresAt`。技能：`defaultVersion`、`createdAt`、`updatedAt`。批处理：`inputFileId`、`inputFileExpiresAt`。

## 已知限制与警告

- 【事实】Responses：`topK`、`seed`、`presencePenalty`、`frequencyPenalty`、`stopSequences` 产生 `unsupported` 警告并被丢弃；推理模型丢弃 `temperature`、`topP`（`reasoningEffort: none` 且模型支持采样参数时保留）；GPT-6 及以后的推理模型丢弃 `logprobs`；非推理模型上的 `reasoning*` 选项产生警告；`serviceTier: flex` 只对 o3、o4-mini、GPT-5 及以后生效，`priority`/`fast` 只对 gpt-4*、GPT-5（`nano`、`chat` 除外）及 o3 以后生效，否则警告并移除。
- 【事实】Responses 文件部件：图像接受 URL、字节、文件 ID；其他媒体类型只接受 `application/pdf` 字节与任意 URL，否则返回 `UnsupportedFunctionality`（`passThroughUnsupportedFiles: true` 时原样发送）；`FileData::Text` 只在匹配 `file_id_prefixes` 时作为文件 ID。
- 【事实】Chat：`topK` 不支持；推理模型丢弃 `temperature`、`topP`、`logprobs`、`frequencyPenalty`、`presencePenalty`、`logitBias`，`maxOutputTokens` 写入 `max_completion_tokens`；`gpt-4o-search-preview*` 丢弃 `temperature`；文件部件只接受图像与 `audio/wav`、`audio/mp3`、`audio/mpeg`、`application/pdf`，`text/*` 返回 `UnsupportedFunctionality`。
- 【事实】Completions：`tools`、`toolChoice`、`responseFormat`、`topK` 产生警告；工具消息与工具调用部件返回 `UnsupportedFunctionality`；非首位的系统消息返回 `InvalidPrompt`。
- 【事实】语音：不支持的 `outputFormat` 回退到 `mp3` 并警告；`language` 不支持。转写：对 `gpt-realtime-whisper*` 调用 `do_generate` 返回 `UnsupportedFunctionality`；对其他模型调用 `do_stream` 同样返回 `UnsupportedFunctionality`；`gpt-4o-transcribe-diarize` 默认 `chunking_strategy: auto` 与 `diarized_json`。
- 【决策】流式调用中 `StreamPart::Error` 是终止部件：错误之前未关闭的部件先收到 end 部件，错误之后不再产出 `Finish`。依据：见实现指南第 9 节。
- 【决策】服务器在产出任何输出之前返回的错误帧使 `do_stream` 以 `ProviderError::ApiCall` 失败（状态码由错误码或类型推断：`rate_limit`/`quota`→429、`authentication`→401、`permission`→403、`not_found`→404、`invalid_request`→400、`overloaded`→503、`timeout`→504，其余 500）。依据：见实现指南第 9 节。
- 【事实】批处理结果只在批次进入终态后可读，进行中的批次返回 `InvalidArgument`；`BatchRequest::Image` 返回 `UnsupportedFunctionality`。
- 【事实】文件引用的键为供应商 `name`，其他键返回 `NoSuchProviderReference`。
- 【事实】实时模型的音频增量以 base64 传输，解码失败返回 `InvalidResponseData`。

## Fixture 清单

fixture 位于 `crates/providers/ferrin-openai/tests/fixtures/<area>/`，由 `tests/suite/*.rs` 通过 `ferrin_testing::FixtureServer` 回放；流式用例以 `-stream` 后缀命名。

| 区域 | 用例 | 覆盖 |
| --- | --- | --- |
| `responses` | `text-basic`、`tool-call`、`reasoning`、`error-400`、`error-429`、`text-basic-stream`、`tool-call-stream`、`reasoning-stream`、`error-early`、`error-late` | 文本、工具调用、推理摘要与加密内容、用量、错误映射、流式契约、早期与晚期错误帧 |
| `chat` | `text-basic`（含 `url_citation`）、`tool-call`、`error-401`、`text-basic-stream`、`tool-call-stream`、`error-early` | 文本、来源、工具调用增量、用量与预测 token 元数据、错误 |
| `completion` | `text-basic`、`text-basic-stream` | 文本与用量 |
| `embedding` | `basic` | 向量与用量 |
| `image` | `generate` | base64 解码、元数据、用量 |
| `transcription` | `verbose`、`diarized`、`words` | 分段、语言映射、说话人元数据、multipart 字段 |
| `files` | `upload`、`get`、`delete` | multipart 上传、元数据、下载、删除 |
| `skills` | `upload` | multipart `files[]` 与结果映射 |
| `batch` | `file-upload`、`create`、`retrieve-pending`、`retrieve-completed`、`retrieve-failed`、`cancel`、`list`、`output.jsonl`、`errors.jsonl` | JSONL 请求体、状态映射、结果流、取消与分页 |
| `realtime` | `client-secret` | 临时密钥与会话配置请求体 |

【待验证】（PV-031）以上 fixture 依据供应商公开 API 文档的响应 schema 手工编写；`record-fixture` 实现后需用真实响应重新录制。

【事实】（2026-09-14，真实凭据验证）`store` 为真（默认）时，多步调用把上一轮的助手消息与供应商执行的工具项以 `item_reference` 回传（节省请求体）；某第三方 OpenAI 兼容代理端点对含 `item_reference` 的请求返回 502，去掉引用项后同一请求成功。对不保存响应项的端点，应设置供应商选项 `{"openai": {"store": false}}`，此时 `ferrin-openai` 回传完整项而不使用引用（`responses/convert_prompt.rs`）。真实 OpenAI 端点未在本次验证中测试。

【决策】 Responses 设置 `conversation` 不表示本地工具结果已上传：工具消息始终发送函数、自定义及 provider-defined 工具输出。来源：`responses/convert_tool_results.rs`；回归测试 `conversation_sends_new_local_tool_results`（2026-09-15）。

【决策】 每个 `openai.custom` 工具独立将应用别名映射到 `args.name`，用于调用、结果、强制选择及回包名称；工具类型 `custom` 不是函数名。来源：`responses/convert_tools.rs`；回归测试 `custom_tool_aliases_roundtrip_calls_results_and_choice`（2026-09-15）。

【事实】本地 `openai.shell` 结果使用 `shell_call_output`，`output` 数组包含 stdout/stderr/outcome，并将 `outcome.exitCode` 转为 `exit_code`；旧版 `openai.local_shell` 保留 `local_shell_call_output`。来源：`responses/convert_tool_results.rs`；回归测试 `local_shell_outputs_use_the_matching_api_generation`（2026-09-15）。
