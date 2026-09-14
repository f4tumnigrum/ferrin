# Other modalities and resource interfaces

**English** | [Chinese](../zh-CN/01-architecture/11-other-modalities.md)

This document defines core functions beyond text generation. All follow the same pattern: builder input, retries, cancellation, headers, `provider_options`, and results containing `warnings`, `response` metadata, and `provider_metadata`.

## 1. Embeddings

[Decision] `embed` handles one value; `embed_many` handles multiple values:

- Split inputs by `max_embeddings_per_call` and `max_input_bytes_per_call`. If `supports_parallel_calls`, run with `max_parallel_calls` (unlimited by default); otherwise run sequentially.
- Retry each call, merge results in input order, and sum usage.
- Results contain `embeddings`, `usage {tokens}`, `responses[]`, `provider_metadata`, and `warnings`.

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

[Decision] Embeddings use `Vec<f32>`. Major providers' arrays need only 32-bit precision, halving memory; providers needing higher precision may expose raw values in metadata.

Provide `ferrin_core::embed::cosine_similarity(&[f32], &[f32]) -> f32`. (2026-09-13: implemented with `Result<f32, Error>`; see section 13.)

## 2. Image generation

[Decision] `generate_image` defaults to `n = 1`. Split by configured/model `max_images_per_call` (default 1) and execute concurrently with retries. Empty results are retryable unless the provider says otherwise; all-empty output returns `NoImageGenerated` with `responses`. Return images (bytes/media types), `warnings`, `responses`, and provider metadata. Chunking hides provider per-request image limits from callers.

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

## 3. Speech synthesis

[Decision] `generate_speech` accepts `text`, `voice`, `output_format`, `instructions`, `speed`, and `language`. Return audio (bytes, media type, format), `warnings`, `responses`, and provider metadata; missing audio is `NoSpeechGenerated`.

```rust
pub fn generate_speech(model: impl Into<SpeechModelRef>, text: impl Into<String>) -> GenerateSpeech;
pub struct GeneratedAudio { pub data: Bytes, pub media_type: MediaType, pub format: Option<String> }
```

## 4. Transcription

[Decision] `transcribe` accepts `audio` bytes or a URL fetched by the downloader. Return `text`, `segments[] {text, start_second, end_second}`, `language`, duration in seconds, `warnings`, `responses`, and provider metadata; empty `text` is `NoTranscriptGenerated`. `stream_transcribe` requires model `do_stream` and emits transcription deltas.

```rust
pub fn transcribe(model: impl Into<TranscriptionModelRef>, audio: AudioInput) -> Transcribe;   // AudioInput = Bytes | Url
pub fn stream_transcribe(model: impl Into<TranscriptionModelRef>, audio: AudioInput) -> StreamTranscribe;
```

## 5. Reranking

[Decision] `rerank` accepts model, query, documents, and `top_n`; return descending-score `ranking[] {original_index, score, document}`, `warnings`, `response`, and provider metadata (implemented signatures in section 13).

```rust
pub fn rerank<D: Into<RerankDocument> + Clone>(model: impl Into<RerankingModelRef>, query: impl Into<String>, documents: Vec<D>) -> Rerank<D>;
pub struct RerankResult<D> { pub ranking: Vec<Ranked<D>>, pub usage: RerankUsage, pub warnings: Vec<Warning>, pub response: ResponseMetadata, pub provider_metadata: Option<ProviderMetadata> }
```

`RerankDocument` is a string or JSON object.

## 6. Video generation

[Decision] `generate_video` supports synchronous `do_generate` or asynchronous `do_start`/`do_status`. Polling configuration or a webhook factory selects asynchronous operation; timeout bounds both polling and webhook waiting. If unsupported, warn and fall back to `do_generate`; if neither form exists, return an error. Video often takes minutes, so long-running operation APIs are standard and synchronous APIs are a convenience.

```rust
pub fn generate_video(model: impl Into<VideoModelRef>, prompt: impl Into<String>) -> GenerateVideo;

impl GenerateVideo {
    pub fn poll(self, config: PollConfig) -> Self;                         // interval, max_attempts
    pub fn webhook(self, factory: Arc<dyn WebhookFactory>) -> Self;
    pub fn timeout(self, total: Duration) -> Self;
}
```

Applications implement `WebhookFactory`, providing a callback URL and a future completing on callback receipt. Ferrin does not provide an HTTP server.

## 7. File and skill uploads

[Decision] `upload_file` accepts a files interface, data, media type, filename, and provider options, returning a provider `reference`, metadata, and warnings. `upload_skill` has the same structure. References become prompt file parts and avoid retransmitting large files.

```rust
pub fn upload_file(files: impl Into<FilesRef>, data: Bytes) -> UploadFile;
impl UploadFile { pub fn media_type(self, mt: MediaType) -> Self; pub fn filename(self, name: impl Into<String>) -> Self; }
pub struct UploadFileResult { pub provider_reference: ProviderReference, pub provider_metadata: Option<ProviderMetadata>, pub warnings: Vec<Warning> }
```

Use uploaded `provider_reference` directly as `FileSource::Reference` in messages.

## 8. Batches

[Decision] Provide `start_batch` (convert generation-style requests to a provider job, returning ID/status), `get_batch_status` (normalized state), `get_batch_results` (stream terminal `succeeded`/`failed`/`cancelled` items; successful text items contain generation-style step results), `cancel_batch` (unsupported providers return `UnsupportedFunctionality`), and `list_batches`.

```rust
pub fn start_batch(batch: impl Into<BatchRef>, requests: Vec<BatchRequest>) -> StartBatch;
pub async fn get_batch_status(batch: impl Into<BatchRef>, batch_id: &BatchId) -> Result<BatchStatus, Error>;
pub fn get_batch_results(batch: impl Into<BatchRef>, batch_id: &BatchId) -> impl Stream<Item = Result<BatchResultItem, Error>>;
pub async fn cancel_batch(batch: impl Into<BatchRef>, batch_id: &BatchId) -> Result<BatchStatus, Error>;
pub async fn list_batches(batch: impl Into<BatchRef>, page: ListBatchesPage) -> Result<BatchList, Error>;
```

`BatchRequest` reuses the settings portion of `GenerateText`, excluding hooks and cancellation.

## 9. Realtime sessions

[Decision] `RealtimeModel` establishes WebSocket sessions with provider endpoints, sends normalized client events (audio, text, tool results, session updates), receives normalized server events (transcription, audio deltas, tool calls, errors), maintains session state, and converts tool sets to provider realtime definitions. Bidirectional endpoints such as OpenAI Realtime cannot be represented by request/response or one-way streaming APIs.

[Decision] Feature-gated `ferrin_core::realtime` provides `RealtimeSession` using `tokio-tungstenite`, with `send(RealtimeClientEvent)`, `events() -> impl Stream<Item = RealtimeServerEvent>`, `tools(ToolSet)` for automatic local execution, and `close()`. No browser transport. (Revised 2026-09-13: events yield `Result<RealtimeServerEvent, Error>` and `tools` are configured before connection; see section 13 and [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 3.)

## 10. Speech translation

[Decision] `SpeechTranslationModel` is streaming-only: audio input streams produce translated text events (`SpeechTranslationStreamPart`). Callers can collect complete output, so a separate non-streaming entry point is unnecessary.

```rust
pub fn stream_speech_translation(model: impl Into<SpeechTranslationModelRef>, audio: impl Stream<Item = Bytes> + Send + 'static) -> StreamSpeechTranslation;
```

## 11. Shared result metadata

All non-text results contain:

```rust
pub struct ResponseMetadata {
    pub timestamp: DateTime<Utc>,
    pub model_id: ModelId,
    pub headers: Option<Headers>,
    pub body: Option<JsonValue>,
}
```

## 12. Verification items

- [Decision] (PV-011) Measure input size with `str::len()` (UTF-8 bytes). Start a new embedding chunk when the current chunk is nonempty and its count reaches `max_embeddings_per_call` or adding an item exceeds `max_input_bytes_per_call`. Reject nonpositive limits; send a single oversized item alone. Byte counts provide a conservative bound without a tokenizer.
- [Decision] (PV-012) Realtime event set. Server events (the original inventory calls this 22): `session-created`, `session-updated`, `speech-started`, `speech-stopped`, `audio-committed`, `conversation-item-added`, `input-transcription-completed`, `response-created`, `response-done`, `output-item-added`, `output-item-done`, `content-part-added`, `content-part-done`, `audio-delta`, `audio-done`, `audio-transcript-delta`, `audio-transcript-done`, `text-delta`, `text-done`, `function-call-arguments-delta`, `function-call-arguments-done`, `error`, and `custom`. Eight client events: `session-update`, `input-audio-append`, `input-audio-commit`, `input-audio-clear`, `conversation-item-create`, `conversation-item-truncate`, `response-create`, `response-cancel`. Define `RealtimeServerEvent`/`RealtimeClientEvent` accordingly; `custom` carries `serde_json::Value`. This covers OpenAI Realtime sessions, audio, transcription, text, and function calls, with provider-specific passthrough.

## 13. Implementation record (2026-09-13)

This section records actual `ferrin-core` signatures and behavior; deviations from sections 1–10 are marked `[Decision]`. All functions return `IntoFuture` builders with `headers`/`header`, `provider_options`/`provider_option`, `cancellation`, `timeout(Duration)` (`TimeoutScope::Total`), and `telemetry`. Model calls also offer `retry(RetryPolicy)`/`max_retries`. Headers append `ferrin/<version>`.

### 13.1 Shared rules

- [Decision] Each call creates `ferrin.modality` with `gen_ai.operation.name` ([ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 5). No `on_start`/`on_end` lifecycle hooks; integrations use `Telemetry::on_embed_*`, `on_rerank_*`, and `on_error`. Single-call modalities have no steps or tool execution, so telemetry events suffice.
- [Decision] Merge metadata shallowly by provider key, concatenating arrays and letting later non-array values win (`modality::merge_provider_metadata`). Preserve per-call arrays such as `images`; do not sum provider-specific fields such as costs.
- [Fact] All `ferrin_spec::ResponseMetadata` fields (`id`, `timestamp`, `model_id`, `headers`, `body`) are optional, superseding the required `timestamp`/model ID in the section 11 draft.

### 13.2 Embeddings

- [Fact] `embed(model, value: impl Into<String>) -> Embed`; `embed_many(model, values: impl IntoIterator<Item: Into<String>>) -> EmbedMany`, with `max_parallel_calls(n)` requiring at least 1. Results: `EmbedResult { value, embedding, usage: EmbeddingUsage { tokens: Option<u64> }, warnings, response, provider_metadata }` and `EmbedManyResult { values, embeddings, usage, warnings, responses, provider_metadata }`.
- [Decision] Add `EmbeddingModel::max_input_bytes_per_call() -> Option<usize>`, default `None`. Chunk per PV-011 using byte lengths; `Some(0)` returns `Error::InvalidArgument`. Any missing per-call `usage` makes total `tokens` `None`. Embedding count mismatches return `ProviderError::InvalidResponseData`.
- [Decision] `cosine_similarity(a, b) -> Result<f32, Error>` returns `Error::InvalidArgument { argument: "vectors" }` for unequal lengths and `0.0` for a zero norm. Caller mistakes return errors rather than panic.

### 13.3 Images

- [Fact] `generate_image(model, prompt) -> GenerateImage` offers `prompt`, `n`, `max_images_per_call`, `size`, `aspect_ratio`, `seed`, `files(Vec<ImageFile>)`, `file`, and `mask(ImageFile)`. `edit_image(model, files: Vec<ImageFile>)` edits without a `prompt`. `GenerateImageResult { images: Vec<GeneratedImage { data: Bytes, media_type: MediaType, provider_metadata }>, calls: Vec<ImageCall>, warnings, responses, provider_metadata, usage: ImageUsage }`; `image()` returns the first image.
- [Decision] `GeneratedImage.media_type` is required: detect unspecified types from magic bytes, falling back to `image/png`, the common provider default, for convenient file saving. Per-image metadata comes from provider `images[index]`.
- [Fact] Empty results enter retries unless `is_retryable: Some(false)`; all-empty results return `Error::NoImageGenerated { responses }`. Reject `n == 0` or `max_images_per_call == Some(0)` with `Error::InvalidArgument`.

### 13.4 Speech and transcription

- [Fact] `generate_speech(model, text)` offers `voice`, `output_format`, `instructions`, `speed`, and `language`, returning `GenerateSpeechResult { audio: GeneratedAudio { data, media_type, format: String }, warnings, request, responses, provider_metadata }`. Format is the media subtype, except `audio/mpeg` → `mp3`. Detect missing media types, falling back to `audio/mpeg`; empty audio is `Error::NoSpeechGenerated`.
- [Fact] `transcribe(model, audio: impl Into<AudioInput>)` accepts `AudioInput::{Bytes, Url}`, `media_type(..)`, and `download(Arc<dyn DownloadFn>)`. Download URLs; media type precedence is caller, download response, magic bytes, then `audio/wav`. Empty text is `Error::NoTranscriptGenerated`.
- [Decision] `stream_transcribe(model, audio: impl Stream<Item = Bytes> + Send + 'static, input_audio_format: AudioFormat) -> StreamTranscribe` offers `include_raw_chunks()` and no retries. `StreamTranscribeResult { request, response }` exposes `parts()`/`into_parts()`, delta-only `text_stream()`, and `consume() -> TranscribeResult` (missing `Finish` or empty text returns `NoTranscriptGenerated`). Unsupported models return `ProviderError::UnsupportedFunctionality`. Required input format is positional to prevent omission.

### 13.5 Reranking

- [Decision] `rerank<D: Into<RerankDocument> + Clone + Send + 'static>(model, query, documents: Vec<D>) -> Rerank<D>` offers `top_n` and returns `RerankResult<D> { ranking: Vec<Ranked<D> { original_index, score, document }>, warnings, response, provider_metadata }`. No `usage`: the specification result lacks it, and reranking APIs generally do not report token `usage`.
- [Fact] Empty documents return empty results without a model call, timestamped now. Mixed text/object documents return `Error::InvalidArgument { argument: "documents" }`; out-of-range indexes return `ProviderError::InvalidResponseData`.

### 13.6 Video

- [Decision] `generate_video(model, prompt)` offers `n`, `max_videos_per_call`, `aspect_ratio`, `resolution`, `duration`, `fps`, `seed`, `image`, `frame_images`, `input_references`, `generate_audio`, `poll(PollConfig { interval: 5 s, timeout: 600 s, max_attempts: None })`, `webhook(WebhookFactory)`, and `download`. Poll timeout bounds both polling and webhook waiting, returning `Error::Timeout { scope: Total }`. `Error` status returns `Error::Other`; a webhook leaving status `Pending` is an error. Defaults cover typical generation times; `Duration` avoids ambiguous integer units.
- [Fact] Each logical start request adds `idempotency-key: ferrin_vid_<id>` unless supplied by the caller. Unsupported `poll`/`webhook` modes warn and fall back to `do_generate`; no supported mode returns `ProviderError::UnsupportedFunctionality`. Download URL videos; media type precedence is provider (except `application/octet-stream`), download response, detection, then `video/mp4`. All-empty output is `Error::NoVideoGenerated { responses }`.

### 13.7 Files, skills, and batches

- [Fact] `upload_file(files, data: impl Into<UploadData>)` offers `media_type`/`filename`, without retries. Default types: text → `text/plain`; streams → `application/octet-stream`; bytes use detection then text classification of the first 512 bytes. Also provide `get_file_metadata`, `download_file`, `delete_file` (unsupported models return `UnsupportedFunctionality`), and `upload_skill(skills, files: Vec<SkillFile>)` with `display_title`.
- [Decision] Core batches use `BatchRequest::{Text(Box<TextBatchRequest>), Image(Box<ImageBatchRequest>)}`. `TextBatchRequest::new(id, model_id)` offers `system`/`prompt`/`messages`/`tools`/`tool_choice`/`active_tools`/`tool_order`/`tools_context`/`settings`/`response_format`. `ImageBatchRequest::new(id, model_id, prompt)` offers `n`/`size`/`aspect_ratio`/`seed`/`files`/`mask`/`provider_options`. Separate builders avoid meaningless hooks/cancellation methods; boxing balances variant sizes.
- [Fact] `start_batch(batch, requests) -> StartBatch` offers `webhook_url`/`download`, checks nonempty unique IDs and consistent same-name tool definitions, and processes text through `standardize` → `CallSettings::validate` → `prepare_tools` → `convert_to_prompt`, using batch `supported_urls` for passthrough. Also provide `get_batch_status(batch, id)`, `get_batch_results(batch, id)` (`tools(ToolSet)` parses result calls; items are `BatchResultItem::{Text(Box<BatchItem<TextBatchResult>>), Image(Box<BatchItem<ImageBatchResult>>)}`), `cancel_batch`, and `list_batches(batch)` with `limit`/`cursor`. Unsupported cancel/list returns `UnsupportedFunctionality`.

### 13.8 Realtime sessions

- [Decision] ([ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 3) `realtime_session(model) -> RealtimeSessionBuilder` offers `client_secret`, `expires_after_seconds`, `config`, `instructions`, `voice`, `tools(ToolSet)`, `tools_context`, `cancellation`, and `event_buffer`; `connect()` or `.await` returns `RealtimeSession`. Create missing secrets with `do_create_client_secret`; put `websocket_config(token, url)` subprotocols in `Sec-WebSocket-Protocol`; immediately send a serialized `SessionUpdate` after connecting.
- [Decision] `RealtimeSession: Stream<Item = Result<RealtimeServerEvent, Error>>` offers `handle() -> RealtimeHandle` (cloneable `send`, `send_raw`, `send_text`, `add_tool_output`, `request_response`, `close`), `next_event`, `events`, and `close()` (wait up to 5 s for the connection task). Transport, parsing, and tool failures yield `Err`; the stream ends after connection closure.
- [Fact] On `FunctionCallArgumentsDone`, known executable tools parse/validate arguments, execute in-session, and submit `FunctionCallOutput`. Tools without executors wait for application `add_tool_output`; unknown tools yield `Err(Error::NoSuchTool)`, invalid arguments yield `Err(Error::InvalidToolInput)`. After all calls in a response have outputs and `ResponseDone` arrives, send one `ResponseCreate`. Reply to `health_check_response` before parsing events when present.
- [Decision] Use `tokio-tungstenite` `rustls-tls-native-roots` for system trust, without custom connectors, aligning with HTTP trust policy without a direct core `rustls-platform-verifier` dependency.
- [Fact] `realtime_tool_definitions(&ToolSet, tools_context) -> Vec<RealtimeToolDefinition>` converts function/dynamic tools only, skipping provider tools.

### 13.9 Speech translation

- [Decision] `stream_speech_translation(model, audio: impl Stream<Item = Bytes> + Send + 'static, input_audio_format: AudioFormat, target_language) -> StreamSpeechTranslation` offers `source_language`, `output_audio_format`, and `include_raw_chunks`, with no retries. Return specification `SpeechTranslationStreamResult`; empty target language is `Error::InvalidArgument`. Required format and target language are positional.
