# Provider specification

**English** | [Chinese](../zh-CN/01-architecture/04-provider-spec.md)

`ferrin-spec` defines the traits and data types provider adapters must implement. This layer contains no HTTP, wire-format, or provider-specific logic.

## 1. Specification version

[Fact] SDKs in dynamically typed languages commonly attach a specification version to each model interface and upgrade older instances at runtime, allowing adapters with different versions to coexist. Compile-time type checking makes this mechanism unnecessary.

[Decision] Ferrin does not support concurrent specification versions. Each breaking `ferrin-spec` release is a specification upgrade, bound by adapter Cargo version constraints. Export `pub const SPEC_VERSION: &str = env!("CARGO_PKG_VERSION");` for diagnostics. Rust checks specification compatibility between adapters and the core at compile time, eliminating runtime version fields and upgrade adapters.

## 2. Trait shape

[Decision] Define two trait layers for each model capability:

1. Implementation trait: native async semantics for adapter authors.
2. Object trait: `Dyn` prefix, boxed futures, object-safe, automatically supplied by a blanket implementation.

```rust
use std::future::Future;

pub trait LanguageModel: Send + Sync + 'static {
    fn provider(&self) -> &ProviderId;
    fn model_id(&self) -> &ModelId;

    /// URL patterns (by media type) the provider can fetch itself.
    /// Files whose URL does not match are downloaded by the core and inlined.
    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send;

    fn do_generate(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<GenerateResult, ProviderError>> + Send;

    fn do_stream(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<StreamResult, ProviderError>> + Send;
}

pub trait DynLanguageModel: Send + Sync + 'static {
    fn provider(&self) -> &ProviderId;
    fn model_id(&self) -> &ModelId;
    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls>;
    fn do_generate(&self, options: CallOptions) -> BoxFuture<'_, Result<GenerateResult, ProviderError>>;
    fn do_stream(&self, options: CallOptions) -> BoxFuture<'_, Result<StreamResult, ProviderError>>;
}

impl<T: LanguageModel> DynLanguageModel for T { /* boxes each call */ }

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;
```

See [Overall architecture](01-overall-architecture.md), section 5. The core and middleware hold `Arc<dyn DynLanguageModel>`; `ferrin_spec::dynamic` provides aliases such as `pub type LanguageModelRef = Arc<dyn DynLanguageModel>`.

`supported_urls` is async because some providers need a remote capability lookup to determine supported URL patterns.

## 3. Language model

### 3.1 Call options

[Decision] `CallOptions` covers sampling (maximum output tokens, temperature, top-p, top-k, presence/frequency penalties, stop sequences, seed), response format (`text` | `json {schema?, name?, description?}`), tools and tool choice (`auto` | `none` | `required` | a named tool), raw chunk inclusion, cancellation, headers, reasoning level (`provider-default` | `none` | `minimal` | `low` | `medium` | `high` | `xhigh`), and provider options. This is the union of major providers' parameters; `unsupported` options produce warnings instead of errors (section 7, contract 1).

```rust
#[derive(Debug, Clone)]
pub struct CallOptions {
    pub prompt: Prompt,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
    pub seed: Option<u64>,
    pub response_format: Option<ResponseFormat>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub include_raw_chunks: bool,
    pub reasoning: ReasoningEffort,          // default ProviderDefault
    pub headers: Headers,
    pub provider_options: ProviderOptions,
    pub cancellation: CancellationToken,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponseFormat {
    Text,
    Json { #[serde(default)] schema: Option<JsonValue>, #[serde(default)] name: Option<String>, #[serde(default)] description: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolChoice { Auto, None, Required, Tool { tool_name: ToolName } }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEffort { #[default] ProviderDefault, None, Minimal, Low, Medium, High, XHigh }
```

`CallOptions` does not derive `Serialize` because it contains a cancellation token. Fixture recording uses `CallOptions::to_recordable()` for serializable snapshots.

### 3.2 Tool definitions

[Decision] Function tools carry `name`, `description?`, `input_schema` (JSON Schema), `strict?`, `input_examples?`, and `provider_options?`. Provider tools carry `id` (`<provider>.<name>`), `name`, and `args` (a JSON object). Applications define and execute function tools locally; providers define and execute tools such as web search and code execution, needing only an identifier and arguments.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolDefinition {
    Function {
        name: ToolName,
        #[serde(default)] description: Option<String>,
        input_schema: JsonValue,
        #[serde(default)] strict: Option<bool>,
        #[serde(default)] input_examples: Vec<JsonObject>,
        #[serde(default)] provider_options: Option<ProviderOptions>,
    },
    Provider {
        id: String,                 // "<provider>.<tool>"
        name: ToolName,
        args: JsonObject,
    },
}
```

### 3.3 Results

[Decision] Non-streaming results carry `content`, `finish_reason`, `usage`, `provider_metadata?`, `request? {body?}`, `response? {id?, timestamp?, model_id?, headers?, body?}`, and `warnings`. Streaming results carry `stream`, `request? {body?}`, and `response? {headers?}`; other information arrives as `response-metadata` and `finish` events. Usage and `finish` reason cannot be known until the `stream` ends.

```rust
pub struct GenerateResult {
    pub content: Vec<Content>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub provider_metadata: Option<ProviderMetadata>,
    pub request: RequestMetadata,        // { body: Option<JsonValue> }
    pub response: ResponseMetadata,      // { id, timestamp, model_id, headers, body }
    pub warnings: Vec<Warning>,
}

pub struct StreamResult {
    pub stream: BoxStream<'static, StreamPart>,
    pub request: RequestMetadata,
    pub response: ResponseMetadata,      // headers only at this point
}
```

[Decision] `StreamResult::stream` yields `StreamPart`, not `Result<StreamPart, _>`; provider errors are `StreamPart::Error` events. This lets the resilience stage implement stream retries and `on_error` on one event path, without distinguishing item errors from termination causes. Adapters convert fatal transport failures into an `Error` event and end the stream.

### 3.4 Stream ordering contract

[Decision] Streams start with `stream-start` and end with `finish`. Each `text-delta` lies between `text-start`/`text-end` for its ID. Tool input follows `tool-input-start`, zero or more `tool-input-delta`, `tool-input-end`, then `tool-call`. Provider part IDs need only be unique within one call; the core remaps collisions across steps. Explicit boundaries support interleaved parts; provider ID generation is outside the core's control.

[Decision] `ferrin-testing` provides `StreamContractChecker` to assert this ordering in adapter tests; the core enables the same checks in debug builds.

## 4. Other model interfaces

The following table summarizes all specification interfaces and their methods. Complete signatures are in `ferrin-spec` rustdoc.

| Trait | Methods | Notes |
| --- | --- | --- |
| `EmbeddingModel` | `max_embeddings_per_call() -> Option<usize>`, `supports_parallel_calls() -> bool`, `do_embed(EmbedOptions{values, cancellation, headers, provider_options}) -> EmbedResult{embeddings, usage{tokens}, provider_metadata, response, warnings}` | The core chunks input using `max_embeddings_per_call` |
| `ImageModel` | `max_images_per_call() -> Option<usize>`, `do_generate(ImageOptions{prompt, n, size, aspect_ratio, seed, files?, mask?, ...}) -> ImageResult{images, warnings, response, provider_metadata}` | One call may return multiple images |
| `SpeechModel` | `do_generate(SpeechOptions{text, voice, output_format, instructions, speed, language, ...}) -> SpeechResult{audio, warnings, request, response, provider_metadata}` | Returns audio bytes and media type |
| `TranscriptionModel` | `do_generate(TranscriptionOptions{audio, media_type, ...}) -> TranscriptionResult{text, segments, language, duration_in_seconds, ...}`; optional `do_stream` | Streaming transcription is optional |
| `RerankingModel` | `do_rerank(RerankOptions{query, documents, top_n, ...}) -> RerankResult{ranking[{index, relevance_score}], provider_metadata, warnings, response}` | Descending relevance order |
| `VideoModel` | Synchronous `do_generate`, or asynchronous `do_start`/`do_status`/optional `handle_webhook` | See the decision below |
| `Files` | `upload_file(UploadFileOptions{data, media_type, filename, provider_options}) -> UploadFileResult{provider_reference, provider_metadata, warnings}`; optional `get_file_metadata`, `download_file`, `delete_file` | Uploads return provider references for file parts |
| `Skills` | `upload_skill(...) -> {provider_reference, ...}` | Same structure as file uploads |
| `Batch` | `start`, `status`, streaming `results`, optional `cancel`, `list` | Results stream item by item |
| `RealtimeModel` | WebSocket sessions: connect, send events, receive normalized events, obtain client secrets | See [Other modalities](11-other-modalities.md) for events |
| `SpeechTranslationModel` | Streaming-only `do_stream(...)` | No non-streaming form |

[Decision] One `VideoModel` trait covers synchronous and asynchronous forms. `do_generate`, `do_start`/`do_status`, and `handle_webhook` default to `UnsupportedFunctionality`, with `supports_generate()`, `supports_operations()`, and `supports_webhook()` queries; adapters implement at least one group. Polling, timeouts, and webhook waiting belong only in core `generate_video`, keeping `tokio::time` out of `ferrin-spec` and centralizing cancellation and polling policy. Optional methods use `fn supports_x(&self) -> bool` plus an unsupported default, rather than `Option<fn>`: Rust traits have no optional methods, so explicit queries replace runtime method-existence checks.

## 5. Provider trait

[Decision] `Provider` requires explicit language, embedding and image lookup implementations; implementing a lookup does not imply support, and unsupported kinds return `NoSuchModelError::unsupported_kind`. Other model lookups default to that error; files, skills, realtime and batch service accessors default to `None`. This lets providers such as reranking-only Voyage satisfy one trait without claiming unsupported model families. Sources: `crates/ferrin-spec/src/provider.rs` and [ADR 0025](../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md), checked 2026-09-17.

```rust
pub trait Provider: Send + Sync + 'static {
    fn provider_id(&self) -> &ProviderId;

    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError>;
    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError>;
    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError>;

    fn transcription_model(&self, model_id: &str) -> Result<TranscriptionModelRef, NoSuchModelError> {
        Err(NoSuchModelError::unsupported_kind(self.provider_id(), model_id, ModelKind::Transcription))
    }
    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> { /* same */ }
    fn reranking_model(&self, model_id: &str) -> Result<RerankingModelRef, NoSuchModelError> { /* same */ }
    fn video_model(&self, model_id: &str) -> Result<VideoModelRef, NoSuchModelError> { /* same */ }
    fn speech_translation_model(&self, model_id: &str) -> Result<SpeechTranslationModelRef, NoSuchModelError> { /* same */ }
    fn realtime(&self) -> Option<RealtimeFactoryRef> { None }
    fn files(&self) -> Option<FilesRef> { None }
    fn skills(&self) -> Option<SkillsRef> { None }
    fn batch(&self) -> Option<BatchRef> { None }
}
```

[Decision] Model factories return synchronously. Credentials are needed only on the first request, so loading them during request construction keeps factories free of I/O and reports missing keys at the actual call site.

## 6. Specification errors

[Decision] `ProviderError` is an enum (variants below). `ApiCallError` retains URL, request body, status, response headers/body, retryability, and structured data. By default, statuses 408, 409, 429, and ≥500 are retryable. Distinct variants let the core retry, fall back, or return immediately; provider documentation identifies these statuses as transient.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    #[error(transparent)] ApiCall(#[from] ApiCallError),
    #[error(transparent)] EmptyResponseBody(#[from] EmptyResponseBodyError),
    #[error(transparent)] InvalidArgument(#[from] InvalidArgumentError),
    #[error(transparent)] InvalidPrompt(#[from] InvalidPromptError),
    #[error(transparent)] InvalidResponseData(#[from] InvalidResponseDataError),
    #[error(transparent)] JsonParse(#[from] JsonParseError),
    #[error(transparent)] LoadApiKey(#[from] LoadApiKeyError),
    #[error(transparent)] LoadSetting(#[from] LoadSettingError),
    #[error(transparent)] NoContentGenerated(#[from] NoContentGeneratedError),
    #[error(transparent)] NoSuchModel(#[from] NoSuchModelError),
    #[error(transparent)] NoSuchProviderReference(#[from] NoSuchProviderReferenceError),
    #[error(transparent)] TooManyEmbeddingValues(#[from] TooManyEmbeddingValuesForCallError),
    #[error(transparent)] TypeValidation(#[from] TypeValidationError),
    #[error(transparent)] UnsupportedFunctionality(#[from] UnsupportedFunctionalityError),
    #[error("operation cancelled")] Cancelled,
    #[error(transparent)] Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ApiCallError {
    pub message: String,
    pub url: Url,
    pub request_body: Option<JsonValue>,
    pub status_code: Option<StatusCode>,
    pub response_headers: Option<Headers>,
    pub response_body: Option<String>,
    pub is_retryable: bool,
    pub data: Option<JsonValue>,
    #[source] pub cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```

`ProviderError::is_retryable()` returns the stored flag only for `ApiCall`, and `false` otherwise. See [Error model](12-error-model.md).

[Fact] Implementation on 2026-09-13: payloads for `ApiCall`, `InvalidPrompt`, `InvalidResponseData`, `JsonParse`, `NoSuchModel`, `NoSuchProviderReference`, and `TooManyEmbeddingValues` use `Box<...>` with automatic boxing in `From` implementations. `TypeValidationError` context is boxed, keeping `size_of::<ProviderError>() <= 128` (`large-error-threshold` in `clippy.toml`), enforced with `const_assert!` in tests.

[Decision] Added `Cancelled` on 2026-09-13 for cancellation-token aborts. Its `kind_name()` is `"cancelled"` and it is not retryable. See [HTTP transport and security](14-http-and-security.md), section 2.

## 7. Adapter contract checklist

Adapters must meet these requirements:

1. Unsupported options in `do_generate`/`do_stream` produce `Warning::Unsupported` and are ignored, rather than causing errors (for example, Anthropic Messages lacks `frequency_penalty`, `presence_penalty`, and `seed`).
2. Read only the adapter's own `provider_options` key, derived from its configured `name`.
3. Pass tool-call `input` through as the original provider JSON string, without parsing.
4. Start with `StreamStart` and end with `Finish`; on failure, emit `Error` and end the stream.
5. Preserve original information in `usage.raw` and `provider_metadata`; use `None` for unmappable standard fields.
6. `request.body` is the sent JSON body; non-streaming `response.body` is the raw response.
7. Abort the HTTP request and end the stream when cancellation fires.
8. Read no process state other than environment variables, accessed through `ferrin_provider_util::settings`.
