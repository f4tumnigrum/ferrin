# Google Generative AI（`ferrin-google`）

[English](../../providers/google.md) | **简体中文**

`ferrin-google` 是 Google Generative AI（Gemini API）的供应商适配器 crate（L3，见[crate 划分](../01-architecture/02-crates.md)）。本文记录 2026-09-14 实现的能力、设置、供应商选项与元数据、限制以及 fixture 清单；实现结构见[Provider 适配器实现指南第 12 节](../01-architecture/17-provider-implementation-guide.md#12-实现记录2026-09-14ferrin-google)。

供应商 ID 按模型族区分：语言模型 `<name>.generative-ai`，语音 `<name>.speech`，转写 `<name>.transcription`，批处理 `<name>.batch`，实时 `<name>.realtime`；嵌入、图像、视频与文件使用不带后缀的 `<name>`。`name` 默认 `google`（`GoogleSettings::name` 可覆盖）；`provider_options` 与 `provider_metadata` 始终读取/写入 `google` 键，`name` 与之不同时同时读取 `name` 键（后者覆盖前者）并把结果级元数据复制到 `name` 键下。

## 能力矩阵

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| 语言模型：`generateContent` / `streamGenerateContent` | 已实现 | `GoogleLanguageModel`，`POST /models/{model}:generateContent` 与 `:streamGenerateContent?alt=sse`；`GoogleProvider::language_model`（别名 `chat`）与 `Provider::language_model` 返回此族。源码 `src/language_model.rs`、`src/request.rs`、`src/convert_prompt.rs`、`src/output.rs`、`src/stream.rs`、`src/json_accumulator.rs`。 |
| 工具调用 / 供应商工具 | 已实现 | 函数声明（JSON Schema 经 `json_schema::convert_json_schema_to_openapi_schema` 转为 OpenAPI 子集，递归 `$ref` 回退为 `parametersJsonSchema`）、`ToolChoice` 全部变体（`functionCallingConfig.mode` 为 `AUTO`/`VALIDATED`/`ANY`/`NONE`，`Tool` 变体写入 `allowedFunctionNames`）；供应商工具 `google.google_search`、`google.enterprise_web_search`、`google.url_context`、`google.code_execution`、`google.file_search`、`google.vertex_rag_store`、`google.google_maps`（工厂 `GoogleTools`，源码 `src/tools.rs`；线格式见 `src/prepare_tools.rs`）。 |
| 结构化输出 | 已实现 | `ResponseFormat::Json` 写入 `responseMimeType: application/json`，带 schema 且 `structuredOutputs` 非 `false` 时写入转换后的 `responseSchema`。 |
| 推理 | 已实现 | Gemini 3 及以后写入 `thinkingConfig.thinkingLevel`（`minimal`→模型最低档、`low`、`medium`、`high`、`xhigh`→`high`；`ReasoningEffort::None`→最低档），Gemini 2.5 按 `map_reasoning_to_budget` 写入 `thinkingBudget`（上限 Pro 32768、其余 24576；`None`→0）。响应的 `thought: true` 部件映射为推理部件，`thoughtSignature` 保留于部件元数据。 |
| 引用 | 已实现 | `groundingMetadata.groundingChunks` 的 `web`、`image`、`retrievedContext`、`maps` 映射为 `Source::Url`/`Source::Document`；流式中同一 URL 只产出一次。 |
| 嵌入 | 已实现 | `GoogleEmbeddingModel`：单值 `:embedContent`，多值 `:batchEmbedContents`，每次最多 100 个值；`outputDimensionality`、`taskType`、多模态 `content`。 |
| 图像 | 已实现 | `GoogleImageModel`：基于 Gemini 图像模型的 `generateContent`（`responseModalities: ["IMAGE"]`），支持 `aspectRatio`、`imageConfig`、参考图像文件与 `googleSearch` 工具；`size`、`mask`、`n > 1` 见限制。 |
| 语音 | 已实现 | `GoogleSpeechModel`：`generateContent` 的 `AUDIO` 模态，默认声音 `Kore`，输出 WAV（默认）或原始 PCM（`outputFormat: pcm`），`multiSpeakerVoiceConfig`。 |
| 转写 | 已实现 | `GoogleTranscriptionModel`：Interactions API 单次转写；`realtime` 为 `-live` ID 启用 Live `do_stream`。 |
| 视频 | 已实现 | `GoogleVideoModel`：`:predictLongRunning` 启动 + 操作轮询（`do_start`/`do_status`），首帧、末帧、参考图像、`aspectRatio`、`resolution`、`durationSeconds`、`seed`；`do_generate` 返回 `UnsupportedFunctionality`。 |
| 文件 | 部分 | `GoogleFiles`：可恢复上传（`/upload/v1beta/files`，两次请求）并轮询至 `ACTIVE`；元数据（`GET /files/{id}`）与删除（`DELETE /files/{id}`）；下载未实现（`supports_download_file` 返回 `false`）。 |
| 批处理 | 已实现 | `GoogleBatch`：`:batchGenerateContent`（内联请求，≥ 20 MB 时改为上传 JSONL 文件）、状态、结果（内联响应或 `responsesFile` 下载的 JSON 行）、取消、列表。接受 `BatchRequest::Text` 与 `BatchRequest::Image`（图像请求经 `GoogleImageModel::prepare_call` 转为语言模型请求）。 |
| 实时（Live API） | 已实现 | `GoogleRealtimeModel`/`GoogleRealtimeFactory`：临时令牌（`POST /v1alpha/auth_tokens`）、WebSocket 会话配置（`setup`）与双向事件映射。 |
| Interactions 语言模型 | 已实现 | `interactions(model_id)`，单次/SSE、状态回放、后台轮询与断线续流。 |
| 语音翻译 | `realtime` 下已实现 | `speech_translation(model_id)`，目标语言文本与 24 kHz PCM 输出。 |
| 重排 | 无 | Gemini API 无对应端点；沿用 `Provider` 的默认实现。 |

## 设置与环境变量

【事实】`create_google(GoogleSettings)`（`src/lib.rs`）：

| 设置 | 环境变量 | 行为 |
| --- | --- | --- |
| `base_url` | 无 | 默认 `https://generativelanguage.googleapis.com/v1beta`；尾部斜杠被去除；无效 URL 时 `create_google` 立即失败。上传、下载与令牌端点使用该 URL 的 origin（`/upload/v1beta/files`、`/download/v1beta/...`、`/v1alpha/auth_tokens`）；WebSocket URL 去掉末尾的 `v1beta`/`v1alpha` 段后拼接 `/ws/<service>` 并把 scheme 改为 `wss`（`http` 改为 `ws`）。 |
| `api_key` | `GOOGLE_GENERATIVE_AI_API_KEY` | 写入 `x-goog-api-key`；首次请求时读取，缺失时该请求以 `ProviderError::LoadApiKey` 失败。 |
| `headers` | 无 | 附加到每个请求；调用级 `headers` 覆盖同名头。 |
| `name` | 无 | 供应商 ID 前缀、附加选项键与文件引用键（默认 `google`）。 |
| `transport` / `id_generator` | 无 | 默认共享的 `reqwest` 传输与随机 ID 生成器（无 ID 的工具调用、来源、批次显示名）。 |

【事实】每个请求的 `user-agent` 追加 `ferrin-google/<crate 版本>`（`config::USER_AGENT`）。可恢复上传的第二次请求（上传 URL）与临时令牌请求不携带 `x-goog-api-key`：前者由上传 URL 自带授权，后者把密钥作为查询参数 `key` 发送。

## 供应商选项（`provider_options["google"]`）

【决策】七个已有工具工厂采用参考实现的对象输入和输出 schema。空工具 schema 接受对象并移除未声明字段；代码执行仅保留声明的字符串字段。来源：本地 AI SDK `6c6c221` 的 `packages/google/src/tool` 和 `tests/suite/tools.rs` 解析回归。仅验证本地校验行为。

【决策】模态选项校验对齐本地 AI SDK `6c6c221` 的 schema：嵌入任务类型使用文档中的枚举，多模态嵌入条目为非空的文本、内联数据或文件数据列表；语音说话人条目必须包含说话人名与预置音色名；图像 grounding 和配置值必须为对象。无效输入在 HTTP 前失败，嵌入响应缺少必需数组时不会自动补空数组。来源：本地参考的 `packages/google/src/google-{embedding,speech,image}-model-options.ts`；本地回归仅验证转换。

【决策】语音响应与参考 speech schema 一样接受缺失或 null 的内联音频媒体类型、数据字段，并选用第一个非空音频载荷。缺少媒体类型时使用默认 24 kHz 采样率，元数据 `mimeType` 为 null。来源：本地 AI SDK `google-speech-api.ts` 与 `google-speech-model.ts`；仅经本地 fixture 验证。

【决策】批处理状态在总数一致时将缺少的 pending/successful/failed 计数视为零，匹配 `convertGoogleRequestCounts`。视频下载 URL 仅在同源时使用解析后的配置 API-key 请求头，包括配置层的请求头覆盖。来源：本地 AI SDK `google-batch.ts` 与 `google-video-model.ts`；凭据隔离保持强制要求。

【决策】视频请求转换省略值为零的 `seed`、`duration`，与参考适配器的真值判断一致；需要在线路中发送这些字段时应使用非零值。来源：本地 AI SDK `6c6c221` 的 `google-video-model.ts`。

【决策】选项键为 camelCase，未知键按参考 Zod 对象解析规则忽略，已知字段的非法值返回 `ProviderError::InvalidArgument`。完整 schema 见 `src/options.rs`。来源：本地 AI SDK `6c6c221` 的选项 schema。

【事实】语言模型：`responseModalities [TEXT | IMAGE]`、`thinkingConfig {thinkingBudget, includeThoughts, thinkingLevel}`（与 `ReasoningEffort` 推导的值合并，显式值优先于推导值中未设置的字段）、`cachedContent`、`structuredOutputs`（默认 `true`）、`safetySettings [{category, threshold}]` 或 `threshold`（对 `HARM_CATEGORY_HATE_SPEECH`、`HARM_CATEGORY_DANGEROUS_CONTENT`、`HARM_CATEGORY_HARASSMENT`、`HARM_CATEGORY_SEXUALLY_EXPLICIT` 四类统一设置）、`audioTimestamp`、`labels {..}`、`mediaResolution`、`imageConfig {aspectRatio, imageSize, personGeneration, prominentPeople, imageOutputOptions}`、`retrievalConfig {latLng}`（写入 `toolConfig.retrievalConfig`）、`serviceTier`（`standard`/`flex`/`priority`）；`streamFunctionCallArguments`、`sharedRequestType`、`requestType` 为 Vertex AI 选项，被忽略并警告。

【事实】部件级选项（`src/options.rs::PartOptions`）：`thoughtSignature`（回放到 `functionCall`、文本与文件部件）、`thought`（助手文件标记为推理文件）、`serverToolCallId` 与 `serverToolType`（把工具调用/结果回放为 `toolCall`/`toolResponse` 部件）。函数工具选项：`strict`（任一函数为 `strict` 时 `functionCallingConfig.mode` 为 `VALIDATED`）。

【事实】嵌入：`outputDimensionality`、`taskType`、`content`（每个值对应一组额外的多模态部件或 `null`，长度须与值数量一致）。图像：`imageConfig`、`googleSearch`（转为 `google.google_search` 工具），其余 `google` 键透传到语言模型选项。语音：`multiSpeakerVoiceConfig`。转写：`languageCodes`、`customVocabulary`、`wordTimestamp`、`diarization`、`mode`（`SMART`/`VERBATIM`）。视频：`personGeneration`、`negativePrompt`、`referenceImages`（只在有帧图像时读取；无帧图像时 `input_references` 作为 `referenceImages` 发送），其余 `google` 键透传到 `parameters`。文件：`displayName`、`pollIntervalMs`（默认 2000）、`pollTimeoutMs`（默认 300000）。实时：`google.translationConfig` 并入 `generationConfig`，其余 `google` 键展开到 `setup`。

## 供应商元数据（`provider_metadata["google"]`）

【事实】结果级（语言模型）：`promptFeedback`、`groundingMetadata`、`urlContextMetadata`、`safetyRatings`、`usageMetadata`（原始用量对象）、`finishMessage`、`serviceTier`（无则 `null`）；`name` 非 `google` 时同一对象复制到 `name` 键。

【事实】部件级：文本、推理、文件与函数调用部件 `thoughtSignature`；供应商执行的工具调用（`toolCall` 部件）`serverToolCallId`、`serverToolType` 与 `thoughtSignature`，映射为工具名 `server:<toolType>`、`provider_executed: true`、`dynamic: true` 的调用与结果；`executableCode`/`codeExecutionResult` 映射为工具名 `code_execution`（或用户为 `google.code_execution` 起的别名）的供应商执行调用与结果。

【事实】用量：`input.total = promptTokenCount`，`input.cache_read = cachedContentTokenCount`，`input.no_cache = promptTokenCount − cachedContentTokenCount`，`output.text = candidatesTokenCount`，`output.reasoning = thoughtsTokenCount`，`output.total` 为两者之和。结束原因：`STOP`→`stop`（有客户端工具调用时 `tool-calls`）、`MAX_TOKENS`→`length`、`SAFETY`/`RECITATION`/`BLOCKLIST`/`PROHIBITED_CONTENT`/`SPII`/`IMAGE_SAFETY`→`content-filter`、`MALFORMED_FUNCTION_CALL`→`error`，其余 `other`；提示被拦截（无候选、`promptFeedback.blockReason`）时结束原因为 `content-filter` 并保留原始原因。

【事实】其他模态：图像 `images: [{}]`（每张一项，保留位置）；语音 `sampleRate`、`mimeType`；转写 `usage`；视频 `videos: [{uri}]`；文件引用 `{google: uri, <name>: uri}` 与元数据 `name`、`displayName`、`mimeType`、`sizeBytes`、`state`、`uri`、`createTime`、`updateTime`、`expirationTime`、`sha256Hash`；批处理启动经文件输入时 `inputFileId`、`inputFileExpiresAt`，失败项 `promptFeedback.blockReason`。

## 已知限制与警告

- 【事实】`frequencyPenalty`、`presencePenalty` 在 Gemini 2.5 上产生 `unsupported` 警告并被丢弃，其他模型照常发送；系统消息只允许出现在对话开头，否则返回 `UnsupportedFunctionality`。
- 【事实】文件部件：字节转为 `inlineData`，URL 与文件引用转为 `fileData`（文本内容按 `text/plain` 内联）；助手消息中的 URL 文件返回 `UnsupportedFunctionality`；引用键非 `google`/`name` 时返回 `NoSuchProviderReference`。`supported_urls` 覆盖 Files API URI（公共端点与配置的 `base_url`）与 YouTube 链接，Gemini 2.0 以外的 Gemini 模型另外接受 22 种媒体类型的任意 HTTPS URL（`language_model::EXTERNAL_URL_MEDIA_TYPES`）。
- 【事实】工具结果：`Content` 输出中的文件只接受字节与 data URL（其余产生警告并忽略）；Gemini 3 及以后把文件放入 `functionResponse.parts`，旧模型追加独立的 `inlineData` 与说明文本部件；错误输出以 `content` 字段发送，`ExecutionDenied` 的 `content` 为其 `reason`（缺省 `Tool call execution denied.`）。
- 【事实】Gemini 3 及以后回放的助手消息中若没有任何带 `thoughtSignature` 的函数调用，所有函数调用写入哨兵签名 `skip_thought_signature_validator` 并产生一条 `other` 警告；有签名调用时未签名的调用原样发送。
- 【事实】函数工具与供应商工具混用：Gemini 3 及以后同时发送并写入 `toolConfig.includeServerSideToolInvocations: true`（`tool_choice` 缺省时 `mode: VALIDATED`）；旧模型只发送供应商工具并警告。`googleSearch`、`enterpriseWebSearch`、`urlContext`、`codeExecution` 需要 Gemini 2 及以后（或 `nano-banana` 模型），`fileSearch` 需要 Gemini 2.5 及以后，否则警告并丢弃；`vertex_rag_store` 在 Gemini API 上产生 `other` 警告。
- 【事实】JSON Schema 转换：只支持指向根级 `$defs`/`definitions` 直接子项的 `$ref`（内联展开），递归引用在函数参数中回退为 `parametersJsonSchema`、在 `responseSchema` 中返回 `UnsupportedFunctionality`；混合类型的 `enum` 返回 `UnsupportedFunctionality`；`type: object` 且无属性的根 schema 不发送。
- 【事实】流式：文本与推理块 ID 为递增整数，函数调用 ID 缺省由 `id_generator` 生成；`partialArgs`（Vertex 流式参数）经 `JsonAccumulator` 还原为 JSON 文本增量。Gemini 流没有流内错误帧，HTTP 错误在流开始前以 `ProviderError::ApiCall` 报告；含 `retry-after` 的 429 标记为可重试。
- 【事实】图像：非 `gemini-` 前缀的模型 ID、`mask` 与 `n > 1` 返回 `InvalidArgument`；`size` 产生警告（改用 `aspectRatio`）；声明的每次调用上限默认为 10，并接受 `with_max_images_per_call` 覆盖值，对齐参考实现，即使 Gemini 仍拒绝 `n > 1`；核心层多图调用必须显式设置 `.max_images_per_call(1)`，以拆分为受支持的单图请求。语音：`speed`、`language` 产生警告并忽略，`instructions` 以 `"<instructions>: <text>"` 前置（`multiSpeakerVoiceConfig` 存在时忽略并警告），`outputFormat: pcm` 返回原始 PCM 并附带说明警告。
- 【事实】视频：`fps`、`generateAudio`、`webhookUrl` 产生警告；参考图像的 `gs://` URI 写入 `gcsUri`，其他 URL 产生警告并忽略；分辨率 1280×720/1920×1080/3840×2160 映射为 `720p`/`1080p`/`4k`，其余以 `WxH` 发送；整数秒时长以整数发送。完成的操作返回 `video/mp4` 的 URL，仅与 `base_url` 同源时附加 `key` 查询参数。
- 【决策】Files 轮询 `state`；`PROCESSING` 超时或 `FAILED` 返回 `ApiCall`。`filename` 不受支持并发出警告；使用 `displayName` 设置 Files API 名称。轮询选项必须为正数，资源名（含点路径段）按本地 AI SDK `google-files.ts` 编码为路径数据。元数据 `sizeBytes` 保留字符串，规范化 `byte_size` 为数字。
- 【事实】批处理：所有请求必须使用同一模型（模型是端点的一部分），否则 `InvalidArgument`；请求 ID 写入 `metadata.key`；输入文件超过 2 GB 返回 `InvalidArgument`；未完成的批次读取结果返回 `InvalidArgument`，已完成但无输出返回 `InvalidResponseData`，失败且无输出返回空流；结果项按 `error.status`（`CANCELLED` 或 code 1 为取消）、被拦截的提示（`prompt_blocked`）、不支持的内容（文件、推理文件、自定义、审批请求→`unsupported_content`）、无法解析的响应（`invalid_response`）分类。
- 【事实】实时：`SessionUpdate` 序列化为 `setup`；`InputAudioAppend` 使用 `audio/pcm;rate=<输入采样率>`（默认 16000）；`toolCall` 事件映射为 `FunctionCallArgumentsDelta` + `Done`；`goAway`、`sessionResumptionUpdate`、`toolCallCancellation`、`generationComplete` 作为 `Custom` 事件透传；没有 Live API 对应物的客户端事件（`InputAudioClear`、`ResponseCreate`、`ResponseCancel`、`ConversationItemTruncate` 与音频消息项）返回 `UnsupportedFunctionality`。
- 【决策】`do_generate` 在视频模型上返回 `UnsupportedFunctionality`，只提供操作式接口（`do_start`/`do_status`）。依据：Veo 端点只有 `predictLongRunning`，同步等待属于核心层的轮询职责。
- 【决策】图像批处理请求中的 `mask` 与 `n > 1` 返回 `InvalidArgument` 而非 `UnsupportedFunctionality`。依据：与 `GoogleImageModel::prepare_call` 的单次调用行为一致，同一输入在两处得到同一错误类型。
- 【决策】批次显示名为 `ferrin-batch-<id>`（`id` 由 `id_generator` 生成），状态元数据只在文件式启动后携带 `inputFileId`/`inputFileExpiresAt`。依据：Gemini 批次没有其他稳定的可读标识；文件 ID 是调用方清理输入文件所需的唯一信息。
- 【决策】安全文件下载由核心层负责，`downloadToolResultFiles` 仍不属于供应商范围。[ADR 0023](../04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md) 取代此前对 Interactions 和 Live 音频的排除。

## Fixture 清单

fixture 位于 `crates/providers/ferrin-google/tests/fixtures/<area>/`，由 `tests/suite/*.rs` 通过 `ferrin_testing::FixtureServer` 回放；`.chunks.txt` 每行一个 `data:` 事件（`ferrin_testing::encode_events_file` 格式，事件内的反斜杠与换行已转义）。

| 区域 | 用例 | 覆盖 |
| --- | --- | --- |
| `generate` | `text`、`tool-call`、`reasoning`、`grounding`、`code-execution`、`server-tool`、`blocked`、`inline-image`、`error-429` | 文本与用量、函数调用、思考签名、来源映射、代码执行、服务器工具调用、被拦截的提示、内联图像、错误映射与 `retry-after` |
| `stream` | `text`、`tool-call`、`tool-call-arguments`、`no-args-tool-call`、`reasoning`、`code-execution`、`blocked`、`inline-image` | 流式契约、函数调用（含 `partialArgs` 增量与无参数调用）、推理块、代码执行与来源去重、拦截、文件部件 |
| `embedding` | `single`、`batch` | `embedContent` 与 `batchEmbedContents` 请求体、值数量上限 |
| `speech` | `generate` | PCM 解码、WAV 封装、原始 PCM 输出、元数据 |
| `transcription` | `generate` | Interactions 请求体、`word_info` 分段 |
| `video` | `start`、`status-pending`、`status-done`、`status-error` | 启动请求体、操作状态映射 |
| `files` | `upload-finalize`、`get-active`、`delete` | 可恢复上传的两次请求与轮询、元数据、删除 |
| `batch` | `create`、`status-running`、`status-succeeded-inline`、`status-succeeded-file`、`status-failed`、`cancel`、`list`、`results.jsonl` | 请求体、状态与计数映射、内联与文件结果流、结果项分类、取消与分页 |
| `realtime` | `auth-token` | 临时令牌请求与过期时间 |

请求体、提示转换、工具线格式、schema 转换与流式部件序列以 `insta` 快照记录于 `tests/suite/snapshots/`；测试共 78 个。批处理 ≥ 20 MB 时的 JSONL 上传路径没有 fixture 覆盖。

【待验证】（PV-031）以上 fixture 依据供应商公开 API 文档的响应 schema 手工编写；`record-fixture` 实现后需用真实响应重新录制。

【决策】SSE EOF 仅在收到显式 provider 终止响应或结束原因（包括 Google 提示词拦截）后表示成功；提前 EOF 产生 `InvalidResponseData`，由流驱动关闭开放的内容块，且不把未完成工具参数转换成可执行调用。来源：流 EOF fixture 边界回归测试（2026-09-15）；未进行 live API 验证。

【事实】 显式 `thinkingConfig` 预算与级别优先于通用推理设置，缺失字段仍使用通用映射。回归覆盖：`tests/suite/request.rs::explicit_thinking_fields_override_generic_reasoning`（2026-09-15）。

【事实】 助手文件与推理文件回放保留生成时的 `thoughtSignature`；验证：`tests/suite/prompt.rs::generated_files_replay_their_thought_signatures`（2026-09-15）。

【决策】按照 ADR 0026，Live API 函数输出直接保留所有有效 JSON 值，无效 JSON 替换为 `{}`，对齐参考 `google-realtime-event-mapper.ts`，取代此前标量/文本包装。标准化 clear、response-create、response-cancel、truncate 和 audio-message 事件返回 `null`；Live API 没有对应操作，core 跳过发送。回归：`tests/suite/realtime.rs::{function_outputs_preserve_json_and_replace_invalid_text,client_events_serialize_to_the_wire_format}`（2026-09-17）。

【决策】 Schema 转换把 `true` 映射为不施加约束的 schema（`{}`），对 `false` 返回 `UnsupportedFunctionality`，包括属性、数组项、联合与引用位置；支持的 OpenAPI 子集无法表达拒绝所有值的 schema。回归：`tests/suite/unit.rs::boolean_schemas_keep_their_validation_meaning`（2026-09-15）。

【事实】 可执行代码及其结果携带 `serverToolType: "code_execution"` 元数据，回放为 `executableCode`/`codeExecutionResult` 部件，并支持应用工具别名。回归：`tests/suite/prompt.rs::generated_code_execution_roundtrips_with_its_result_and_alias` 与代码执行流 fixture（2026-09-15）。

【事实】 返回的可恢复上传 URL 在发送文件字节之前经过 `url_policy` 校验：默认只允许 HTTPS 与公网地址，固定解析后的地址，拒绝重定向。上传完成响应遵守字节上限。调用方头仅发送到配置的同源地址或明确的 `credentialed_origins`，且始终移除 `x-goog-api-key`。本地测试端点需要显式 `allow_http().trust_origin(...)`。来源：`src/files.rs`、`tests/suite/security.rs`（2026-09-15）。

【决策】ADR 0023 取代此前对通用 Interactions、Live 流式转写和语音翻译的排除；实现和本地验证记录见 [ADR 0023](../04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md)。

## 实现记录（2026-09-17）：Interactions 与 Live 音频

【决策】Interactions 内置结果步骤回放时保留 JSON 对象、数组和标量原值；客户端 `function_result` 才将 JSON 序列化为文本。MCP 结果同时保留工具名。这保证供应商结果不会误用客户端结果的 wire 序列化规则。来源：本地 AI SDK `6c6c221` 的 Interactions 结果 schema 与 `src/interactions/prompt.rs`；未做在线验证。

【决策】参考请求规范化省略空停止序列，不从通用推理强度推导 Interactions `thinking_level`，需显式使用 `thinkingLevel`。弃用的 `imageConfig` 仅在没有显式图像条目时补入图像响应格式。压缩关联历史时移除对应工具调用已被压缩的结果，保留不相关的新结果；相邻用户文本块以空行合并。来源：本地 AI SDK `6c6c221` 的 Interactions 请求和提示转换器。

【事实】`GoogleInteractionsLanguageModel` 使用 `<name>.interactions`；`language_model` 和 `chat` 保留 generateContent。`google` 和自定义名称下的选项合并，后者优先：`previousInteractionId`、`store`、`agent`、`agentConfig`、`environment`、`background`、`pollingTimeoutMs`（默认 30 分钟）、`thinkingLevel`、`thinkingSummaries`、`responseFormat`、`responseModalities`、`mediaResolution`、`serviceTier` 和 `systemInstruction`。提示系统消息优先于 `systemInstruction` 并给出警告。来源：`src/interactions/request.rs`、请求快照回归。

【事实】Interactions 转换用户文本/文件、助手文本/文件/推理/函数调用、供应商工具回放和工具结果。`signature`、`interactionId`、`stepType` 在标准和自定义键下回放；启用存储时压缩匹配 `previousInteractionId` 的助手历史，保留新工具结果。`store: false` 保留完整历史，与先前 ID 合用时警告。JSON Schema 保留用户属性名称；响应格式转换为下划线 wire 键。来源：`src/interactions/prompt.rs`、请求及回放回归。

【事实】函数以及 Google 搜索/代码执行/URL 上下文/文件搜索/地图/计算机使用/检索/MCP 工具有 Interactions wire 转换，内置别名恢复应用名称。工具转换逐项选择已定义字段、忽略 null，并采用参考实现的计算机使用和检索默认值。远端 `mcp_server` URL 交给 Google 执行，不建立本地连接。未知供应商工具警告并忽略。单次及流式输出包含文本、推理、文件、调用/结果、URL/文档/地图来源、token 计数和服务等级；流式来源去重。来源：本地 AI SDK `6c6c221` 的 `prepare-google-interactions-tools.ts`，以及 `src/interactions/tools.rs`、`output.rs`、`sources.rs` 和 fixture 回归。未经真实供应商验证。

【事实】`start_interaction` 返回初始资源，`get_interaction` 查询资源，`cancel_interaction` 停止资源；使用 `GoogleInteractionOptions` 的请求头/取消并编码 ID 路径段。`do_generate` 每秒轮询进行中的后台/agent 调用，直到配置时限。`do_stream` 创建后台资源后增量读取 `GET /interactions/{id}?stream=true`；断流使用 `last_event_id` 续接并忽略重复边界事件，最多重连两次，遵守同一时限。初始 POST 已终结时立即合成事件。来源：`src/interactions/background.rs`、生命周期回归。

【决策】显式取消后台轮询/流时尝试有界远端取消，清理失败也返回原取消。丢弃流直接关闭自有连接，不启动清理任务；需要丢弃后取消远端任务时，通过 `start_interaction` 保留 ID 并调用 `cancel_interaction`。缺失续传 ID、非法可执行调用、不匹配增量及提前 EOF 关闭失败；终结事件即刻结束流，无需等待连接关闭。来源：生命周期与流边界回归。

【事实】启用 `realtime` 时，Live 转写为 `-live` ID 接受 `audio/pcm` 或 `pcm16`，即 16 kHz、有符号 16 位单声道 PCM。翻译接受相同输入，输出 24 kHz `audio/pcm`，自动识别源语言，要求目标语言并接受 `echoTargetLanguage`。`setupComplete` 后才提交音频；输出保留 usage 和自定义键元数据。连接使用 URL 策略、DNS 固定及有界帧；取消与 drop 释放自有资源。来源：`src/live_audio`、`src/transcription/live.rs`、`src/speech_translation` 及本地 WebSocket 回归。

【决策】音频终结沿用参考适配器：转写在输入 EOF 后允许一秒静默宽限；翻译以一秒 PCM 静音（阈值 128）或轮次完成加宽限终结。无分离后台任务维持已丢弃的流。使用可控本地 WebSocket 和暂停时间覆盖这些边界，不证明官方端点兼容性；PV-031 的真实响应录制要求继续开放。

【决策】ADR 0026 迁移恢复参考图像上限 10；依赖自动单图拆分的既有多图调用必须设置 `.max_images_per_call(1)`，否则适配器在 HTTP 前拒绝产生的 `n > 1` 请求。来源：本地 AI SDK `6c6c221`，`packages/google/src/google-image-model.ts`。

【决策】Interactions 选项规范化在 agent 调用中保留显式供应商响应格式，仅丢弃通用 JSON 结构化输出。各模态格式、agent 配置与远程环境对象只序列化参考实现定义的字段，省略空可选字段与空环境源列表；动态 agent 配置只发送类型。无效格式类型或环境结构在 HTTP 前失败。来源：本地 AI SDK `6c6c221` 的 Interactions 语言模型与选项 schema。
