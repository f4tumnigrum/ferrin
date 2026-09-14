# OpenAI 兼容端点（`ferrin-openai-compatible`）

`ferrin-openai-compatible` 是 OpenAI 兼容端点的通用适配器 crate（L3，见[crate 划分](../01-architecture/02-crates.md)），供第三方端点直接使用，或作为专用供应商 crate 的构件（后者提供自己的名称、错误体结构、元数据提取器与请求体转换器）。本文记录 2026-09-13 实现的能力、设置、供应商选项与元数据、限制以及 fixture 清单；实现结构见[Provider 适配器实现指南第 11 节](../01-architecture/17-provider-implementation-guide.md#11-实现记录2026-09-13ferrin-openai-compatible)。

供应商 ID 形如 `<name>.<family>`，`name` 由 `OpenAiCompatibleSettings::name` 指定（必填，非空且不含 `.`）。`provider_options` 按优先级递增读取 `openai-compatible`（已弃用，产生 `deprecated` 警告）、`openaiCompatible`、`<name>`、camelCase(`<name>`) 四个键，后者覆盖前者；`provider_metadata` 写入 camelCase(`<name>`)（调用方以该键提供选项时）或 `<name>`。

## 能力矩阵

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| 语言模型：Chat Completions（生成/流式） | 已实现 | `OpenAiCompatibleChatLanguageModel`，`POST /chat/completions`；`OpenAiCompatibleProvider::chat` 与 `Provider::language_model` 返回此族。源码 `src/chat/`。 |
| 语言模型：Completions（生成/流式） | 已实现 | `OpenAiCompatibleCompletionLanguageModel`，`POST /completions`；提示转换为 `user:`/`assistant:` 文本格式。源码 `src/completion.rs`。 |
| 工具调用 | 已实现 | 函数工具（`strict` 仅在设置时发送）、`ToolChoice` 全部变体（`auto`/`none`/`required`/`{type: function, function: {name}}`）；供应商工具产生 `unsupported` 警告并被丢弃。 |
| 结构化输出 | 已实现 | `supports_structured_outputs: true` 时 `response_format: {type: json_schema, json_schema: {schema, strict, name, description}}`（`strict` 默认 `true`，可用 `strictJsonSchema` 关闭，`name` 默认 `response`）；否则写入 `{type: json_object}` 并在提供 schema 时警告。 |
| 推理 | 已实现 | 响应的 `reasoning_content`（或 `reasoning`）与 `content` 数组中的 `thinking` 部件映射为推理部件；请求写入 `reasoning_effort`（`reasoningEffort` 选项，或自定义 `ReasoningEffort` 值）。 |
| 嵌入 | 已实现 | `OpenAiCompatibleEmbeddingModel`，`POST /embeddings`，`encoding_format: float`；上限与并行由设置决定。源码 `src/embedding.rs`。 |
| 图像 | 已实现 | `OpenAiCompatibleImageModel`：无输入文件时 `POST /images/generations`（JSON），有输入文件时 multipart `POST /images/edits`；每次最多 10 张。源码 `src/image.rs`。 |
| 引用 / 文件 / 技能 / 批处理 / 语音 / 转写 / 重排 / 视频 / 实时 | 无 | 兼容端点没有统一的这些能力；`Provider` 对应方法沿用默认实现（返回 `None`）。 |
| Responses API | 无 | 见 [ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md)：兼容端点的 Responses 调用使用 `ferrin-openai` 的 `base_url` 与 `name` 设置。 |

## 设置与环境变量

【事实】`create_openai_compatible(OpenAiCompatibleSettings)`（`src/lib.rs`）；除 `name` 与 `base_url` 外均为可选：

| 设置 | 行为 |
| --- | --- |
| `name` | 供应商 ID 前缀与选项键；空白或含 `.` 时返回 `InvalidArgument`。 |
| `base_url` | 必填；尾部斜杠被去除；请求 URL 为 `base_url + 路径`，再附加 `query_params`。 |
| `api_key` | 写入 `authorization: Bearer <key>`。 |
| `api_key_env` | `api_key` 缺省时每次请求读取该环境变量；变量不存在时不发送 `authorization` 头，也不报错（本地端点通常无需密钥）。 |
| `headers` / 调用级 `headers` | 附加到每个请求，调用级覆盖同名头；`user-agent` 追加 `ferrin-openai-compatible/<crate 版本>`（`config::USER_AGENT`）。 |
| `query_params` | 附加到每个请求 URL 的查询参数（如 `api-version`）。 |
| `include_usage` | `true` 时流式请求写入 `stream_options: {include_usage: true}`（Chat 与 Completions）；默认 `false`，因为部分端点拒绝该字段。 |
| `supports_structured_outputs` | 是否发送 `response_format.json_schema`（默认 `false`）。 |
| `supported_urls` | Chat 模型接受为 URL 文件部件的模式（默认无）。 |
| `error_structure` | `ErrorStructure` 实现：从错误体提取消息并可覆盖可重试判断；默认读取 OpenAI 形状的 `{error: {message, type, code}}`。 |
| `metadata_extractor` | `MetadataExtractor` 实现：从 Chat 响应体（非流式）或逐块（流式）提取附加的 `provider_metadata`。 |
| `transform_request_body` | Chat 请求体发送前的转换钩子。 |
| `convert_usage` | Chat 用量的自定义映射（默认见下文）。 |
| `max_embeddings_per_call` / `supports_parallel_calls` | 嵌入模型的每次调用上限（默认 2048）与并行开关（默认 `true`）。 |
| `transport` / `id_generator` | 默认共享的 `reqwest` 传输与随机 ID 生成器（缺少 `id` 的工具调用）。 |

## 供应商选项

【事实】Chat（`src/chat/options.rs`）：`user`、`reasoningEffort`、`textVerbosity`（写入 `verbosity`）、`strictJsonSchema`；`<name>` 与 camelCase(`<name>`) 键下的其他字段原样写入请求体（透传），`openaiCompatible` 键下的未知字段不透传。选项对象不匹配 schema 时返回 `InvalidArgument`。

【事实】Completions（`src/completion.rs`）：`echo`、`logitBias`（写入 `logit_bias`）、`suffix`、`user`；其他字段同样透传。嵌入（`src/embedding.rs`）：`dimensions`、`user`。图像（`src/image.rs`）：`<name>`/camelCase 键下的所有字段作为附加参数写入生成请求体或 multipart 表单字段（`null` 字段在表单中省略）。

【事实】消息与部件级：`provider_options["openaiCompatible"]` 对象的字段展开到对应的线格式对象（系统/用户/助手/工具消息、文本与文件部件、工具调用与工具结果），用于 `name` 等端点专有字段；工具调用部件的 `thoughtSignature`（`<name>`/camelCase 键，回退 `google` 键）写入 `extra_content.google.thought_signature`。

## 供应商元数据

【事实】Chat 结果级：`{[key]: {acceptedPredictionTokens?, rejectedPredictionTokens?}}`（无预测用量时为空对象），再合并 `metadata_extractor` 的输出；工具调用部件带 `extra_content.google.thought_signature` 时写入 `{[key]: {thoughtSignature}}`。嵌入响应体的 `providerMetadata` 字段原样返回。Completions 与图像无元数据。

【事实】用量（`chat::output::convert_usage`）：`input.total = prompt_tokens`、`input.no_cache = prompt_tokens - prompt_tokens_details.cached_tokens`、`input.cache_read = cached_tokens`、`output.total = completion_tokens`、`output.reasoning = completion_tokens_details.reasoning_tokens`、`output.text = completion_tokens - reasoning_tokens`（饱和减法）、`raw` 为原始 `usage` 对象；缺少 `usage` 时为空用量。Completions 只映射输入/输出总数；嵌入返回 `usage.prompt_tokens`；图像返回 `input_tokens`/`output_tokens`/`total_tokens`。

## 已知限制与警告

- 【事实】`topK` 产生 `unsupported` 警告（Chat 与 Completions）；Completions 对 `tools`、`toolChoice`、非文本 `responseFormat` 警告并忽略；图像对 `aspectRatio`（提示使用 `size`）与 `seed` 警告并忽略。
- 【事实】Chat 文件部件：`image/*` 接受字节（`data:` URL，媒体类型按字节探测）与 URL；`video/*` 同样；`audio/*` 只接受字节且限于 `audio/wav`→`wav`、`audio/mp3`/`audio/mpeg`→`mp3`（`input_audio`）；`application/pdf` 只接受字节（`{type: file, file: {filename ?? "document.pdf", file_data}}`）；`text/*` 字节按 UTF-8 解码为文本部件，URL 以字符串形式作为文本发送；供应商引用、文本数据与其他媒体类型返回 `UnsupportedFunctionality`。助手消息中的文件部件产生警告并被忽略。
- 【事实】Completions 提示：只有首条系统消息被接受（作为前缀），其后的系统消息返回 `InvalidPrompt`；工具调用与工具消息返回 `UnsupportedFunctionality`；`stop` 固定包含 `"\nuser:"`。
- 【事实】工具结果：`Text`/`ErrorText` 原文、`ExecutionDenied` 为 `reason` 或 `Tool call execution denied.`、`Json`/`ErrorJson`/`Content` 为 JSON 字符串；审批响应部件被跳过。
- 【事实】流式工具调用增量在 `function.name` 到达前按 `index` 缓冲（部分端点首个增量不含名称），名称到达后一次产出 `ToolInputStart` 与累计的输入增量；流结束时仍无名称的增量产出 `InvalidResponseData` 错误部件。缺少 `id` 的工具调用用 `id_generator` 生成 ID。
- 【决策】流式调用中 `StreamPart::Error` 是终止部件：错误之前未关闭的部件先收到 end 部件，错误之后不再产出 `Finish`。依据：见实现指南第 9 节。
- 【决策】流在没有 `finish_reason` 的情况下结束时产出 `InvalidResponseData` 错误部件（`response stream ended without a finish reason`），不产出带 `error` 结束原因的 `Finish` 部件。依据：见实现指南第 11 节。
- 【决策】服务器在产出任何输出之前返回的 `error` 帧使 `do_stream` 以 `ProviderError::ApiCall` 失败；HTTP 状态由 `error_structure` 的消息与帧内 `code`/`type` 推断：三位数字 `code` 直接作为状态码，`insufficient_quota`/`rate_limit`→429、`authentication`→401、`permission`→403、`not_found`→404、`invalid`/`bad_request`/`context_length`→400、`overload`→503、`timeout`→504，其余 500；408/409/429 与 5xx 可重试（`insufficient_quota` 除外）。输出之后的 `error` 帧作为终止的 `StreamPart::Error` 产出，携带同样推断的状态码。HTTP 错误响应保留服务器状态码，消息由 `error_structure` 提取，可重试性由 `error_structure.is_retryable` 覆盖，否则按状态码判断。
- 【事实】图像编辑的输入文件与遮罩必须是 `FileData::Bytes`（URL 与引用返回 `UnsupportedFunctionality`）；单个文件写入 `image` 字段，多个文件写入 `image[]`；响应的 `b64_json` 解码失败返回 `InvalidResponseData`，媒体类型按字节探测。
- 【事实】嵌入值数量超过 `max_embeddings_per_call` 时在请求前返回 `TooManyEmbeddingValues`。
- 【事实】响应缺少 `choices` 时返回 `InvalidResponseData`（`response did not contain any choices`）。

## Fixture 清单

fixture 位于 `crates/providers/ferrin-openai-compatible/tests/fixtures/<area>/`，由 `tests/suite/*.rs` 通过 `ferrin_testing::FixtureServer` 回放；流式用例以 `-stream` 后缀命名（错误流除外），`.chunks.txt` 每行一个 `data:` 事件（Chat Completions 的 SSE 不带 `event:` 字段），以 `data: [DONE]` 结尾。

| 区域 | 用例 | 覆盖 |
| --- | --- | --- |
| `chat` | `text-basic`、`reasoning`、`tool-call`、`error-401`、`error-custom`、`text-basic-stream`、`reasoning-stream`、`tool-call-stream`、`no-finish-stream`、`error-early`、`error-late` | 文本与缓存/预测用量、`reasoning_content`、工具调用（含缺少 `id` 与 `extra_content` 签名）、HTTP 错误与自定义错误体、流式契约、名称延迟到达的工具调用增量、缺少结束原因、早期与晚期错误帧 |
| `completion` | `text-basic`、`text-basic-stream` | 提示格式、`stop`、用量、流式部件 |
| `embedding` | `basic` | 向量、用量、`providerMetadata` 透传、请求体 |
| `image` | `generate`、`edit` | base64 解码、用量、JSON 生成请求体、multipart 编辑表单 |

请求体、提示转换与流式部件序列以 `insta` 快照记录于 `tests/suite/snapshots/`；测试共 43 个，另覆盖选项键解析、错误帧推断、请求头、查询参数、`transform_request_body`/`convert_usage`/`metadata_extractor`/`error_structure` 钩子。

【待验证】（PV-031）以上 fixture 依据供应商公开 API 文档的响应 schema 手工编写；`record-fixture` 实现后需用真实响应重新录制。
