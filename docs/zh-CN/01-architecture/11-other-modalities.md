# 其他模态与资源接口

[English](../../01-architecture/11-other-modalities.md) | **简体中文**

本文档定义文本生成以外的核心函数。每个函数遵循同一模式：构建器输入、重试策略、取消令牌、请求头、`provider_options`、结果含 `warnings`、`response` 元数据与 `provider_metadata`。

## 1. 嵌入

【决策】`embed`（单值）与 `embed_many`（多值）：

- `embed_many` 按 `max_embeddings_per_call` 与 `max_input_bytes_per_call` 把输入切分为多次调用；`supports_parallel_calls` 为真时以 `max_parallel_calls`（默认无限）并发，否则串行。
- 每次调用应用重试策略；结果按输入顺序合并，用量累加。
- 结果：`embeddings`、`usage {tokens}`、`responses[]`、`provider_metadata`、`warnings`。

```rust
pub fn embed(model: impl Into<EmbeddingModelRef>, value: impl Into<String>) -> Embed;
pub fn embed_many(model: impl Into<EmbeddingModelRef>, values: Vec<String>) -> EmbedMany;

impl EmbedMany {
    pub fn max_parallel_calls(self, n: usize) -> Self;
    pub fn max_retries(self, n: u32) -> Self;
    pub fn cancellation(self, token: CancellationToken) -> Self;
    pub fn provider_options(self, opts: ProviderOptions) -> Self;
}

pub struct EmbedManyResult {
    pub embeddings: Vec<Embedding>,       // Embedding = Vec<f32>
    pub usage: EmbeddingUsage,            // { tokens: Option<u64> }
    pub responses: Vec<ResponseMetadata>,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<Warning>,
}
```

【决策】嵌入向量类型为 `Vec<f32>`。依据：主流供应商返回 32 位精度即可满足的浮点数组，`f32` 减少一半内存；需要更高精度的供应商通过 `provider_metadata` 暴露原始值。

`ferrin_core::embed::cosine_similarity(&[f32], &[f32]) -> f32` 作为辅助函数提供。（2026-09-13：实现返回 `Result<f32, Error>`，见第 13 节。）

## 2. 图像生成

【决策】`generate_image`：`n` 默认 1；按 `max_images_per_call`（参数或模型声明，默认 1）拆分为多次调用并并发执行；每次调用在重试策略内进行，空结果视为可重试错误（供应商标记为不可重试的除外）；全部为空时报 `NoImageGenerated` 错误（携带各次响应）；结果为图像列表（字节与媒体类型）、`warnings`、`responses`、`provider_metadata`。依据：供应商对单次请求的图像数有上限，拆分并发使 `n` 对调用方透明。

```rust
pub fn generate_image(model: impl Into<ImageModelRef>, prompt: impl Into<String>) -> GenerateImage;

impl GenerateImage {
    pub fn n(self, n: u32) -> Self;
    pub fn size(self, size: ImageSize) -> Self;               // "1024x1024"
    pub fn aspect_ratio(self, ratio: AspectRatio) -> Self;    // "16:9"
    pub fn seed(self, seed: u64) -> Self;
    pub fn max_images_per_call(self, n: u32) -> Self;
    pub fn files(self, files: Vec<FilePart>) -> Self;         // reference images
    pub fn mask(self, mask: FilePart) -> Self;
}

pub struct GeneratedImage { pub data: Bytes, pub media_type: MediaType }
```

## 3. 语音合成

【决策】`generate_speech`：输入 `text`、`voice`、`output_format`、`instructions`、`speed`、`language`；结果为音频（字节、媒体类型、格式）、`warnings`、`responses`、`provider_metadata`；无音频时报 `NoSpeechGenerated` 错误。

```rust
pub fn generate_speech(model: impl Into<SpeechModelRef>, text: impl Into<String>) -> GenerateSpeech;
pub struct GeneratedAudio { pub data: Bytes, pub media_type: MediaType, pub format: Option<String> }
```

## 4. 转写

【决策】`transcribe`：`audio` 为字节或 URL（URL 经下载器获取）；结果为 `text`、`segments[] {text, start_second, end_second}`、`language`、`duration_in_seconds`、`warnings`、`responses`、`provider_metadata`；无文本时报 `NoTranscriptGenerated` 错误。`stream_transcribe` 要求模型实现 `do_stream`，输出转写增量事件。

```rust
pub fn transcribe(model: impl Into<TranscriptionModelRef>, audio: AudioInput) -> Transcribe;   // AudioInput = Bytes | Url
pub fn stream_transcribe(model: impl Into<TranscriptionModelRef>, audio: AudioInput) -> StreamTranscribe;
```

## 5. 重排

【决策】`rerank`（模型、查询、文档、`top_n`）：结果为按分数降序的 `ranking[] {original_index, score, document}`、`warnings`、`response`、`provider_metadata`（签名见第 13 节实现记录）。

```rust
pub fn rerank<D: Into<RerankDocument> + Clone>(model: impl Into<RerankingModelRef>, query: impl Into<String>, documents: Vec<D>) -> Rerank<D>;
pub struct RerankResult<D> { pub ranking: Vec<Ranked<D>>, pub usage: RerankUsage, pub warnings: Vec<Warning>, pub response: ResponseMetadata, pub provider_metadata: Option<ProviderMetadata> }
```

`RerankDocument` 为字符串或 JSON 对象。

## 6. 视频生成

【决策】`generate_video`：模型可实现同步 `do_generate` 或异步 `do_start`/`do_status`；提供轮询配置或 Webhook 工厂时走异步流程，超时同时限制轮询总时长与 Webhook 等待时长；模型不支持异步流程但传入轮询/Webhook 配置时记录警告并回退到 `do_generate`；两者皆不支持时报错。依据：视频生成通常耗时数分钟，供应商普遍以长时操作 API 暴露，同步形态只是便利封装。

```rust
pub fn generate_video(model: impl Into<VideoModelRef>, prompt: impl Into<String>) -> GenerateVideo;

impl GenerateVideo {
    pub fn poll(self, config: PollConfig) -> Self;                         // interval, max_attempts
    pub fn webhook(self, factory: Arc<dyn WebhookFactory>) -> Self;
    pub fn timeout(self, total: Duration) -> Self;
}
```

`WebhookFactory` 由应用实现：创建一个可被供应商回调的 URL，并返回一个在回调到达时完成的 Future；Ferrin 不提供 HTTP 服务器。

## 7. 文件与技能上传

【决策】`upload_file`（文件接口、数据、媒体类型、文件名、供应商选项）返回供应商引用、供应商元数据与警告；`upload_skill` 同构。依据：上传得到的引用随后作为文件部件的 `reference` 形态进入 prompt，避免重复传输大文件。

```rust
pub fn upload_file(files: impl Into<FilesRef>, data: Bytes) -> UploadFile;
impl UploadFile { pub fn media_type(self, mt: MediaType) -> Self; pub fn filename(self, name: impl Into<String>) -> Self; }
pub struct UploadFileResult { pub provider_reference: ProviderReference, pub provider_metadata: Option<ProviderMetadata>, pub warnings: Vec<Warning> }
```

上传结果的 `provider_reference` 可直接作为 `FileSource::Reference` 用于消息。

## 8. 批处理

【决策】批处理提供 `start_batch`（把一组 `generate_text` 风格请求转换为供应商批任务，返回批 ID 与状态）、`get_batch_status`（规范化状态）、`get_batch_results`（流式返回终态结果，按 `succeeded`/`failed`/`cancelled` 分类，成功项包含与 `generate_text` 相同形状的步骤结果）、`cancel_batch`（不支持时返回 `UnsupportedFunctionality` 错误）、`list_batches`。

```rust
pub fn start_batch(batch: impl Into<BatchRef>, requests: Vec<BatchRequest>) -> StartBatch;
pub async fn get_batch_status(batch: impl Into<BatchRef>, batch_id: &BatchId) -> Result<BatchStatus, Error>;
pub fn get_batch_results(batch: impl Into<BatchRef>, batch_id: &BatchId) -> impl Stream<Item = Result<BatchResultItem, Error>>;
pub async fn cancel_batch(batch: impl Into<BatchRef>, batch_id: &BatchId) -> Result<BatchStatus, Error>;
pub async fn list_batches(batch: impl Into<BatchRef>, page: ListBatchesPage) -> Result<BatchList, Error>;
```

`BatchRequest` 复用 `GenerateText` 构建器的设置部分（不含回调与取消令牌）。

## 9. 实时会话

【决策】实时会话的能力面：经规范 trait `RealtimeModel` 与供应商实时端点建立 WebSocket 连接，发送标准化客户端事件（音频、文本、工具结果、会话更新），接收标准化服务端事件（转写、音频增量、工具调用、错误），在会话内维护状态，并把工具集转换为供应商的实时工具定义。依据：这是 OpenAI Realtime API 一类实时端点的公共功能面；双向事件流无法用其他模态的请求/响应或单向流接口表达。

【决策】Ferrin 的 `ferrin_core::realtime`（feature `realtime`）提供 `RealtimeSession`（基于 `tokio-tungstenite`），暴露 `send(RealtimeClientEvent)`、`events() -> impl Stream<Item = RealtimeServerEvent>`、`tools(ToolSet)`（本地工具在会话内自动执行并回传结果）、`close()`。不提供浏览器传输。（2026-09-13 修订：事件项为 `Result<RealtimeServerEvent, Error>`，`tools` 为连接前的构建器方法，见第 13 节与 [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 3 项。）

## 10. 语音翻译

【决策】规范 trait `SpeechTranslationModel` 只提供流式接口：输入音频流，输出翻译文本事件（`SpeechTranslationStreamPart`）。依据：语音翻译端点以流式形态提供；需要完整结果的调用方收集流即可，不必另设非流式入口。

```rust
pub fn stream_speech_translation(model: impl Into<SpeechTranslationModelRef>, audio: impl Stream<Item = Bytes> + Send + 'static) -> StreamSpeechTranslation;
```

## 11. 通用结果元数据

所有非文本函数的结果包含：

```rust
pub struct ResponseMetadata {
    pub timestamp: DateTime<Utc>,
    pub model_id: ModelId,
    pub headers: Option<Headers>,
    pub body: Option<JsonValue>,
}
```

## 12. 待验证

- 【决策】（PV-011）`embed_many` 以 `str::len()`（UTF-8 字节数）度量每个输入的大小；分块规则为：当前块非空且（块内条数 ≥ `max_embeddings_per_call` 或 累计字节 + 本条字节 > `max_input_bytes_per_call`）时开启新块；两个上限 ≤ 0 时报错；单条超限的输入仍独占一块发送。依据：供应商按字节或令牌限制单次请求体积，字节数是无需分词器即可计算的保守上界。
- 【决策】（PV-012）实时事件集合。服务端事件 22 种：`session-created`、`session-updated`、`speech-started`、`speech-stopped`、`audio-committed`、`conversation-item-added`、`input-transcription-completed`、`response-created`、`response-done`、`output-item-added`、`output-item-done`、`content-part-added`、`content-part-done`、`audio-delta`、`audio-done`、`audio-transcript-delta`、`audio-transcript-done`、`text-delta`、`text-done`、`function-call-arguments-delta`、`function-call-arguments-done`、`error`、`custom`。客户端事件 8 种：`session-update`、`input-audio-append`、`input-audio-commit`、`input-audio-clear`、`conversation-item-create`、`conversation-item-truncate`、`response-create`、`response-cancel`。`RealtimeServerEvent`/`RealtimeClientEvent` 枚举按此集合定义，`custom` 变体携带 `serde_json::Value`。依据：该集合覆盖 OpenAI Realtime API 的会话、音频、转写、文本与函数调用事件，供应商特有事件以 `custom` 透传。

## 13. 实现记录（2026-09-13）

本节记录 `ferrin-core` 各模态函数的实际签名与行为；与第 1–10 节草案签名不同之处以【决策】标注。所有函数返回实现 `IntoFuture` 的构建器，共用 `headers`/`header`、`provider_options`/`provider_option`、`cancellation`、`timeout(Duration)`（`TimeoutScope::Total`）、`telemetry`；模型调用类函数另有 `retry(RetryPolicy)`/`max_retries`。请求头统一追加 `ferrin/<version>` User-Agent 后缀。

### 13.1 共同规则

- 【决策】每个模态调用创建一个 `ferrin.modality` span，操作名放在 `gen_ai.operation.name` 字段（[ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 5 项）；不提供 `on_start`/`on_end` 生命周期回调，遥测集成只通过 `Telemetry` 的 `on_embed_*`、`on_rerank_*`、`on_error` 事件。依据：非文本模态是单次调用，没有步骤与工具执行，`Telemetry` 事件已足够表达开始、结束与失败。
- 【决策】`provider_metadata` 跨多次调用合并时按供应商键做浅合并，同键下的数组拼接、其他值后者覆盖（`modality::merge_provider_metadata`）。依据：数组字段（如各次调用返回的 `images`）需要保留全部元素，标量字段取最后一次即可；不对费用一类字段求和，因为其语义因供应商而异。
- 【事实】`ResponseMetadata`（`ferrin_spec`）的 `id`、`timestamp`、`model_id`、`headers`、`body` 均为 `Option`（第 11 节草案中的 `timestamp`、`model_id` 非可选写法以规范层实现为准）。

### 13.2 嵌入

- 【事实】`embed(model, value: impl Into<String>) -> Embed`；`embed_many(model, values: impl IntoIterator<Item: Into<String>>) -> EmbedMany`（`max_parallel_calls(n)`，最小 1）。`EmbedResult { value, embedding, usage: EmbeddingUsage { tokens: Option<u64> }, warnings, response, provider_metadata }`，`EmbedManyResult { values, embeddings, usage, warnings, responses, provider_metadata }`。
- 【决策】`ferrin_spec::EmbeddingModel` 增加 `max_input_bytes_per_call() -> Option<usize>`（默认 `None`）；分块按 PV-011 规则以 `str::len()` 计字节；上限为 `Some(0)` 时返回 `Error::InvalidArgument`。任一调用缺少 `usage` 时合计 `tokens` 为 `None`。返回的向量数与输入数不符时报 `ProviderError::InvalidResponseData`。
- 【决策】`cosine_similarity(a, b) -> Result<f32, Error>`：长度不同返回 `Error::InvalidArgument { argument: "vectors" }`，任一范数为 0 返回 `0.0`。依据：长度不同是调用方错误，以 `Result` 而非 panic 表达。

### 13.3 图像

- 【事实】`generate_image(model, prompt) -> GenerateImage`（`prompt`、`n`、`max_images_per_call`、`size`、`aspect_ratio`、`seed`、`files(Vec<ImageFile>)`、`file`、`mask(ImageFile)`）；`edit_image(model, files: Vec<ImageFile>)` 为无 prompt 的编辑入口。结果 `GenerateImageResult { images: Vec<GeneratedImage { data: Bytes, media_type: MediaType, provider_metadata }>, calls: Vec<ImageCall>, warnings, responses, provider_metadata, usage: ImageUsage }`，`image()` 取首张。
- 【决策】`GeneratedImage.media_type` 非可选：供应商未给出时按魔数探测，探测失败取 `image/png`。依据：主流图像模型默认输出 PNG，探测失败时以此兜底比返回 `None` 更便于下游保存文件。每张图像的 `provider_metadata` 取自供应商元数据中 `images[index]`。
- 【事实】空结果作为可重试错误进入重试循环（供应商 `is_retryable: Some(false)` 除外），全部为空时返回 `Error::NoImageGenerated { responses }`；`n == 0` 或 `max_images_per_call == Some(0)` 为 `Error::InvalidArgument`。

### 13.4 语音与转写

- 【事实】`generate_speech(model, text)`（`voice`、`output_format`、`instructions`、`speed`、`language`）返回 `GenerateSpeechResult { audio: GeneratedAudio { data, media_type, format: String }, warnings, request, responses, provider_metadata }`；`format` 取媒体类型子类型，`audio/mpeg` 为 `mp3`；无媒体类型时探测，失败取 `audio/mpeg`；空音频为 `Error::NoSpeechGenerated`。
- 【事实】`transcribe(model, audio: impl Into<AudioInput>)`（`AudioInput::{Bytes, Url}`，`media_type(..)`、`download(Arc<dyn DownloadFn>)`）：URL 经下载函数获取；媒体类型优先级为调用方指定、下载响应、魔数探测、`audio/wav`；空文本为 `Error::NoTranscriptGenerated`。
- 【决策】`stream_transcribe(model, audio: impl Stream<Item = Bytes> + Send + 'static, input_audio_format: AudioFormat) -> StreamTranscribe`（`include_raw_chunks()`，无重试）返回 `StreamTranscribeResult { request, response }` 并提供 `parts()`/`into_parts()`（`TranscriptionStreamPart` 流）、`text_stream()`（仅增量文本）与 `consume() -> TranscribeResult`（折叠为完整结果，缺少 `Finish` 或文本为空时 `NoTranscriptGenerated`）。模型不支持流式时返回 `ProviderError::UnsupportedFunctionality`。依据：流式输入的音频格式是每次调用的必需参数，作为位置参数避免遗漏。

### 13.5 重排

- 【决策】`rerank<D: Into<RerankDocument> + Clone + Send + 'static>(model, query, documents: Vec<D>) -> Rerank<D>`（`top_n`）返回 `RerankResult<D> { ranking: Vec<Ranked<D> { original_index, score, document }>, warnings, response, provider_metadata }`，无 `usage` 字段。依据：`ferrin_spec::RerankingModel` 的结果没有用量字段，重排 API 普遍不返回令牌用量。
- 【事实】文档为空时不调用模型，直接返回空结果（`response.timestamp` 为当前时间）；文本与对象文档混用为 `Error::InvalidArgument { argument: "documents" }`；越界索引为 `ProviderError::InvalidResponseData`。

### 13.6 视频

- 【决策】`generate_video(model, prompt)`（`n`、`max_videos_per_call`、`aspect_ratio`、`resolution`、`duration`、`fps`、`seed`、`image`、`frame_images`、`input_references`、`generate_audio`、`poll(PollConfig { interval: 5 s, timeout: 600 s, max_attempts: None })`、`webhook(WebhookFactory)`、`download`）。`PollConfig.timeout` 同时限制轮询总时长与 Webhook 等待时长，超时报 `Error::Timeout { scope: Total }`；状态为 `Error` 时报 `Error::Other`；Webhook 到达后状态仍为 `Pending` 视为错误。依据：轮询间隔 5 s 与总超时 600 s 覆盖主流视频模型的典型生成时长；以 `Duration` 而非毫秒整数表达避免单位歧义。
- 【事实】每个逻辑启动请求带 `idempotency-key: ferrin_vid_<id>` 头（调用方已设置时不覆盖）；模型不支持异步流程而传入 `poll`/`webhook` 时记录 `unsupported` 警告并回退 `do_generate`；两者皆不支持时报 `ProviderError::UnsupportedFunctionality`；`FileData::Url` 视频经下载函数取回；媒体类型优先级为供应商报告（`application/octet-stream` 除外）、下载响应、探测、`video/mp4`；全部为空时 `Error::NoVideoGenerated { responses }`。

### 13.7 文件、技能与批处理

- 【事实】`upload_file(files, data: impl Into<UploadData>)`（`media_type`、`filename`，无重试）：默认媒体类型 `Text -> text/plain`、`Stream -> application/octet-stream`、`Bytes` 先探测再按前 512 字节判断是否为文本。另有 `get_file_metadata`、`download_file`、`delete_file`（模型不支持时 `UnsupportedFunctionality`）与 `upload_skill(skills, files: Vec<SkillFile>)`（`display_title`）。
- 【决策】批处理请求为核心层类型 `BatchRequest::{Text(Box<TextBatchRequest>), Image(Box<ImageBatchRequest>)}`：`TextBatchRequest::new(id, model_id)` 提供 `system`/`prompt`/`messages`/`tools`/`tool_choice`/`active_tools`/`tool_order`/`tools_context`/`settings`/`response_format`，`ImageBatchRequest::new(id, model_id, prompt)` 提供 `n`/`size`/`aspect_ratio`/`seed`/`files`/`mask`/`provider_options`。依据：批处理请求不含回调与取消令牌，复用 `GenerateText` 构建器会暴露无意义的方法；装箱使枚举变体大小接近。
- 【事实】`start_batch(batch, requests) -> StartBatch`（`webhook_url`、`download`）校验 ID 非空且唯一、同名工具定义一致，文本请求经 `standardize` → `CallSettings::validate` → `prepare_tools` → `convert_to_prompt`（以批处理服务的 `supported_urls` 决定 URL 直传）。`get_batch_status(batch, id)`、`get_batch_results(batch, id)`（`tools(ToolSet)` 用于解析结果中的工具调用；流项 `BatchResultItem::{Text(Box<BatchItem<TextBatchResult>>), Image(Box<BatchItem<ImageBatchResult>>)}`）、`cancel_batch`、`list_batches(batch)`（`limit`、`cursor`）；取消与列表不支持时 `UnsupportedFunctionality`。

### 13.8 实时会话

- 【决策】（[ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 3 项）`realtime_session(model) -> RealtimeSessionBuilder`（`client_secret`、`expires_after_seconds`、`config`、`instructions`、`voice`、`tools(ToolSet)`、`tools_context`、`cancellation`、`event_buffer`），`connect()`（或 `.await`）返回 `RealtimeSession`。未提供 `client_secret` 时通过 `do_create_client_secret` 创建；`websocket_config(token, url)` 给出的子协议写入 `Sec-WebSocket-Protocol`；连接后立即发送经 `serialize_client_event` 序列化的 `SessionUpdate`。
- 【决策】`RealtimeSession: Stream<Item = Result<RealtimeServerEvent, Error>>`，提供 `handle() -> RealtimeHandle`（可克隆：`send`、`send_raw`、`send_text`、`add_tool_output`、`request_response`、`close`）、`next_event`、`events`、`close()`（等待连接任务至多 5 s）。传输错误、无法解析的服务器消息与工具执行失败作为 `Err` 项出现；连接关闭后流结束。
- 【事实】本地工具执行：`FunctionCallArgumentsDone` 事件对应的工具存在且可执行时，解析并校验参数与上下文，解析 `NeedsApproval`，仅在无需审批时在会话内执行，输出以 `FunctionCallOutput` 提交（2026-09-15，`tests/suite/realtime.rs`）。需要审批时通过事件流返回错误，调用保持待处理，应用负责取得批准、执行并通过 `add_tool_output` 提交结果；Realtime 没有内置审批恢复协议，审批解析随会话取消；工具存在但无执行函数时不自动处理（应用调用 `add_tool_output`）；工具不存在为 `Err(Error::NoSuchTool)`；参数不合法为 `Err(Error::InvalidToolInput)`。同一响应内的多个工具调用全部提交输出且收到 `ResponseDone` 后，只发送一次 `ResponseCreate`。`health_check_response` 有返回时先回复再解析事件。
- 【决策】WebSocket TLS 使用 `tokio-tungstenite` 的 `rustls-tls-native-roots`（系统根证书）；不提供自定义连接器。依据：与 HTTP 传输的系统信任库策略一致，且不在 `ferrin-core` 引入 `rustls-platform-verifier` 直接依赖。
- 【事实】`realtime_tool_definitions(&ToolSet, tools_context) -> Vec<RealtimeToolDefinition>` 只转换函数与动态工具，跳过供应商工具。

### 13.9 语音翻译

- 【决策】`stream_speech_translation(model, audio: impl Stream<Item = Bytes> + Send + 'static, input_audio_format: AudioFormat, target_language) -> StreamSpeechTranslation`（`source_language`、`output_audio_format`、`include_raw_chunks`，无重试），返回规范层 `SpeechTranslationStreamResult`；`target_language` 为空为 `Error::InvalidArgument`。依据：目标语言与输入格式是每次调用的必需参数。

【事实】 流式转写和语音翻译从等待 builder 到终止流事件采用同一个总期限，涵盖提供商建立流的过程。建立时超时返回 `Error::Timeout { scope: Total }`，流中超时发出一个 `error_type: "timeout"` 的终止错误事件；调用方取消发出 `error_type: "cancelled"`。完成、超时、取消及丢弃流均取消派生的提供商 token，不取消调用方 token（2026-09-15，`tests/suite/modalities/stream_timeout.rs`）。
