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

【待验证】（PV-031）原有 fixture 依据供应商公开 API 文档的响应 schema 手工编写。2026-09-17 新增四个通过第三方代理录制的 Responses 用例（见下文）；官方 OpenAI 响应及其余用例仍需录制与对照。

## 代理响应录制验证（2026-09-17）

【事实】`responses/recorded-proxy/` 包含四个使用真实凭据访问私有第三方 Responses 端点的录制用例，请求及响应模型均为 `gpt-5.6-sol`，每个用例包含场景、请求、响应/SSE 和元数据文件。最终样本录制于 UTC 08:11:35–08:12:08，使用 `store: false`、推理强度 `low` 和 `max_output_tokens: 512`，均返回 HTTP 200。服务商名称与端点地址不入库，场景通过 `OPENAI_BASE_URL` 读取端点。来源：fixture 元数据和 `tests/suite/responses_recorded.rs`。

【事实】回放将 SDK 生成的请求体与录制请求逐一比较，检查完整上游 `Usage.raw`，对归一化输出/用量建立快照，并验证 SSE 契约及归一化事件序列。四个录制样本的对照结果：

| 用例 | 输出 | 输入 / 输出 token | 缓存输入 | 结束原因 |
| --- | --- | --- | --- | --- |
| `text-basic` | `pong` | 4393 / 5 | 4224 | `stop` |
| `text-basic-stream` | `pong` | 4393 / 5 | 4224 | `stop` |
| `tool-call` | `get_weather({"city":"Berlin"})` | 4432 / 18 | 0 | `tool-calls` |
| `structured-output` | `{"city":"Paris","country":"France"}` | 4426 / 16 | 0 | `stop` |

【事实】与原有手写文本/工具 fixture 相比，这些响应包含额外的 `usage.attribution`、`cache_write_tokens` 字段，以及代理附加的 `instructions`、账户/缓存标识和输出元数据。录制器将响应根及 SSE `response` 根下的 `instructions`、`safety_identifier` 和 `prompt_cache_key` 替换为 `[REDACTED]`；每个元数据文件记录这些指针。输出、用量、事件顺序和请求/响应 ID 均保留。原有合成 fixture 继续覆盖这些录制中未出现的推理及错误分支。

【事实】SSE 样本包含九个事件，从 `response.created` 到 `response.completed`，其中只有一个 `response.output_text.delta`；终止事件的 `output` 缺少先前事件中的内容项 ID。Ferrin 仍能生成完整文本流和终止 `Finish`。JSON 和 SSE 响应将请求中的 512 输出 token 限制回显为 `max_output_tokens: null`，并将 4382 个输入 token 归因于附加指令。这些观察无法证明请求上限得到执行，也无法确定代理实际费用。来源：录制的请求/响应对和归一化快照。

【事实】2026-09-17，七个现有 Responses 在线测试在相同端点/模型及 `{"openai":{"store":false,"reasoningEffort":"low"}}` 配置下通过：四个门面测试（文本、流式、本地工具执行与结果往返、类型化结构化输出）和三个适配器测试（文本、流式、工具调用产出）。来源：nextest run `dcf4adb2-6a4a-4dbd-a979-9260998c7a56`；本次排除 Chat Completions。导出端点、模型、密钥和供应商选项后可复现：

```sh
cargo xtask record-fixture --provider openai --case responses/recorded-proxy/text-basic
INSTA_UPDATE=no just test -E "'test(responses_recorded)'"
just test --run-ignored only -E "'(package(ferrin) | package(ferrin-openai)) & test(live_) & !test(live_chat_)'" --test-threads 1
```

【事实】验证仅适用于该代理及模型别名，不涵盖官方 OpenAI 端点、其他供应商、流式工具参数、流式结构化输出、推理输出、错误、其他模态或原始传输字节边界（录制器保存 SSE 事件边界）。回放不验证脱敏值。PV-031 保持 open。

## 其他实现记录

【事实】（2026-09-14，真实凭据验证）`store` 为真（默认）时，多步调用把上一轮的助手消息与供应商执行的工具项以 `item_reference` 回传（节省请求体）；某第三方 OpenAI 兼容代理端点对含 `item_reference` 的请求返回 502，去掉引用项后同一请求成功。对不保存响应项的端点，应设置供应商选项 `{"openai": {"store": false}}`，此时 `ferrin-openai` 回传完整项而不使用引用（`responses/convert_prompt.rs`）。真实 OpenAI 端点未在本次验证中测试。

【决策】 Responses 设置 `conversation` 不表示本地工具结果已上传：工具消息始终发送函数、自定义及 provider-defined 工具输出。来源：`responses/convert_tool_results.rs`；回归测试 `conversation_sends_new_local_tool_results`（2026-09-15）。

【决策】 每个 `openai.custom` 工具独立将应用别名映射到 `args.name`，用于调用、结果、强制选择及回包名称；工具类型 `custom` 不是函数名。来源：`responses/convert_tools.rs`；回归测试 `custom_tool_aliases_roundtrip_calls_results_and_choice`（2026-09-15）。

【事实】本地 `openai.shell` 结果使用 `shell_call_output`，`output` 数组包含 stdout/stderr/outcome，并将 `outcome.exitCode` 转为 `exit_code`；旧版 `openai.local_shell` 保留 `local_shell_call_output`。来源：`responses/convert_tool_results.rs`；回归测试 `local_shell_outputs_use_the_matching_api_generation`（2026-09-15）。

【事实】Responses 和 Chat 请求构建使用 `OpenAiConfig.name` 解析上传文件引用，与 provider options 键相互独立；独立提示词转换辅助函数仍使用默认名称 `openai`。来源：回归测试 `uploaded_files_roundtrip_with_custom_provider_name`（2026-09-15）。

【决策】Provider 工具参数转换仅重命名已知 API 字段并遍历已知配置对象；headers、metadata、Schema 及未知参数值保持原样，以保留用户字典键和 HTTP 头名称。来源：`responses/convert_tools.rs`；回归测试 `provider_tool_options_preserve_opaque_dictionary_keys`（2026-09-15）。

【决策】SSE EOF 仅在收到显式 provider 终止响应或结束原因（包括 Google 提示词拦截）后表示成功；提前 EOF 产生 `InvalidResponseData`，由流驱动关闭开放的内容块，且不把未完成工具参数转换成可执行调用。来源：流 EOF fixture 边界回归测试（2026-09-15）；未进行 live API 验证。

【事实】DALL-E 图片编辑发送单个 multipart `image` 并显式请求 `response_format=b64_json`；GPT 图片编辑使用 `image[]` 及默认 base64 响应。DALL-E 多张输入图片会在请求前被拒绝。来源：`image/mod.rs` 和 `image_edits_use_model_specific_file_fields_and_response_format`（2026-09-15）。

【事实】Chat 非流式和流式生成均在 `Usage.raw` 保留上游完整 `usage` 对象，包括音频计数及未纳入归一化用量类型的字段。来源：`chat/mod.rs`、`chat/stream.rs`；回归测试 `raw_usage_preserves_unmodeled_fields_in_generate_and_stream`（2026-09-15）。

【决策】Responses/Chat 的函数输入和结构化输出 Schema 在最终 strict 为 true（默认）时使用 `SchemaTransform::OpenAiStrict`。函数 `strict` 覆盖 `strictJsonSchema`；false 保留归一化后的 Schema。严格转换关闭对象、将全部属性设为必需，并使可选受约束值允许 null；无法表示的字典在 HTTP 请求前报错（ADR [0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md)）。来源：`strict_schema` 回归测试（2026-09-15），仅验证请求形态。

【决策】Responses 高级工具往返遵循 [ADR 0022](../04-decisions/2026-09-17-0022-provider-tool-roundtrips.md)：托管程序、搜索和 shell 保留 ID 与回放字段；客户端搜索结果保留协议类型；供应商调用者绑定支持延迟程序结果。未声明的 parallel 包装仅展开已声明函数接收者，并保留有序回放元数据。确定性验证不关闭 PV-031。

【事实】高级工具回归（`responses_advanced.rs`、`responses_parallel.rs`、`tools.rs`，2026-09-17）覆盖生成与流式等价、托管 shell 分类、客户端搜索结果、程序调用者身份、无存储回放、有序完整 parallel 结果组和批处理映射。批处理结果缺少请求函数声明，因此其中的内部 parallel 包装保持未展开。程序化调用的执行拒绝结果在 HTTP 前被拒绝。这些是合成 fixture 检查，不属于供应商实时验证。

【事实】托管 shell 环境类型将 `containerAuto`/`containerReference` 转换为 `container_auto`/`container_reference`；转换网络策略键时保留秘密条目原文。来源：`hosted_shell_environment_uses_wire_types_and_preserves_secret_names`（2026-09-17），仅验证请求形态。
