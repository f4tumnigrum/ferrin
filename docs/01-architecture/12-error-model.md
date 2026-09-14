# Error model

**English** | [Chinese](../zh-CN/01-architecture/12-error-model.md)

## 1. Layers

| Layer | Type | Description |
| --- | --- | --- |
| Specification | `ferrin_spec::error::ProviderError` and concrete error structs | Adapter errors; see [Provider specification](04-provider-spec.md), section 6. |
| Schema | `ferrin_schema::{TypeValidationError, JsonParseError, SchemaError}` | Validation and parsing errors. |
| Tools | `ferrin_tool::ToolError` | Nonfatal execution errors returned to the model. |
| Core | `ferrin_core::Error` | Unified application-facing error enum. |
| MCP | `ferrin_mcp::McpError` | JSON-RPC, transport, and negotiation errors; wrapped as `Error::Mcp` in the core. |

## 2. Core errors

[Fact] The core distinguishes download failures, invalid data, roles, stream parts, approvals and tool input, MCP errors, message conversion, missing image/object/output/speech/transcription results, unspecified output, missing providers/tools, exhausted retries, missing approval calls, repair failures, tool-choice violations, timeouts, and cancellation.

[Decision] Use one `#[non_exhaustive]` `Error` enum with structured fields and a `source()` chain. Runtime type markers are unnecessary because Rust types are unique within a crate version.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error("retries exhausted after {attempts} attempts: {reason:?}")]
    Retry { reason: RetryReason, attempts: u32, errors: Vec<ProviderError> },

    #[error("timeout ({scope:?}) after {elapsed:?}")]
    Timeout { scope: TimeoutScope, elapsed: Duration },

    #[error("operation cancelled")]
    Cancelled,

    #[error("invalid argument `{argument}`: {message}")]
    InvalidArgument { argument: &'static str, message: String },

    #[error("invalid prompt: {message}")]
    InvalidPrompt { message: String },

    #[error("message conversion failed: {message}")]
    MessageConversion { message: String, original_message: Box<Message> },

    #[error("download failed for {url}")]
    Download { url: Url, status_code: Option<StatusCode>, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("invalid data content")]
    InvalidDataContent { #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("no such tool `{tool_name}`")]
    NoSuchTool { tool_name: ToolName, available_tools: Vec<ToolName> },

    #[error("invalid input for tool `{tool_name}`")]
    InvalidToolInput { tool_name: ToolName, tool_input: String, #[source] cause: Box<dyn std::error::Error + Send + Sync> },

    #[error("tool call repair failed")]
    ToolCallRepair { original: Box<Error>, #[source] cause: Box<dyn std::error::Error + Send + Sync> },

    #[error("tool choice violated: expected `{expected}`, got `{actual}`")]
    ToolChoiceViolation { expected: ToolName, actual: ToolName },

    #[error("tool call `{tool_call_id}` not found for approval `{approval_id}`")]
    ToolCallNotFoundForApproval { tool_call_id: ToolCallId, approval_id: ApprovalId },

    #[error("invalid tool approval: {message}")]
    InvalidToolApproval { approval_id: ApprovalId, message: String },

    #[error("no structured output generated")]
    NoObjectGenerated { text: Option<String>, response: ResponseMetadata, usage: Usage, finish_reason: FinishReason, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("no output generated")]
    NoOutputGenerated,

    #[error("no image generated")]
    NoImageGenerated { responses: Vec<ResponseMetadata> },
    #[error("no speech generated")]
    NoSpeechGenerated { responses: Vec<ResponseMetadata> },
    #[error("no transcript generated")]
    NoTranscriptGenerated { responses: Vec<ResponseMetadata> },
    #[error("no video generated")]
    NoVideoGenerated { responses: Vec<ResponseMetadata> },          // 2026-09-13, see §6

    #[error("no such provider `{provider_id}`")]
    NoSuchProvider { provider_id: ProviderId, available_providers: Vec<ProviderId>, model_id: String, model_kind: ModelKind },

    #[error("no default registry configured for model id `{model_id}`")]
    NoDefaultRegistry { model_id: String },

    #[error("invalid stream part: {message}")]
    InvalidStreamPart { message: String },

    #[error(transparent)]
    Mcp(#[from] McpError),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
```

### 2.1 Classification methods

```rust
impl Error {
    pub fn is_retryable(&self) -> bool;     // Provider(ApiCall{is_retryable}) or Retry with retryable last error
    pub fn is_cancelled(&self) -> bool;     // Cancelled, or Timeout caused by caller cancellation
    pub fn status_code(&self) -> Option<StatusCode>;
    pub fn kind(&self) -> ErrorKind;        // stable, serializable coarse category
}
```

`ErrorKind` is a serializable coarse category (`Provider`, `Retry`, `Timeout`, `Cancelled`, `InvalidInput`, `Tool`, `Output`, `NotFound`, `Mcp`, `Other`) for low-cardinality telemetry and log labels.

[Decision] Added specification `ProviderError::Cancelled` on 2026-09-13 ([Provider specification](04-provider-spec.md), section 6). `From<ProviderError> for Error` maps it to core `Cancelled`, so `is_cancelled()` and `kind() == ErrorKind::Cancelled` cover adapter cancellation, and retries stop immediately.

### 2.2 Stream events

Streaming errors arrive as `StreamEvent::Error { error: StreamErrorInfo }`, projected from `Error` into `kind`, `message`, `status_code`, `is_retryable`, and `provider_data`. `Error`-terminated streams return `Err(Error)` through `Completion`.

[Fact] Stream failures can occur after partial output has been delivered, too late to report through the initial return value.

## 3. Tool errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{message}")]
    Message { message: String, #[source] cause: Option<Box<dyn std::error::Error + Send + Sync>> },
    #[error("tool returned error payload")]
    Json { value: JsonValue },
    #[error("tool execution timed out after {0:?}")]
    Timeout(Duration),
    #[error("tool execution cancelled")]
    Cancelled,
}
```

[Decision] Tool errors produce `tool-error` content and `error-text`/`error-json` feedback without terminating generation, allowing the model to correct arguments and retry.

`ToolError::Message` maps to `error-text`; `Json` to `error-json`; `Timeout` to `error-text` including duration; `Cancelled` terminates the invocation as `Error::Cancelled`.

## 4. Error message conventions

- Begin lowercase with no trailing period, following Rust standard library style.
- Never include secrets, headers, or complete request bodies. `ApiCallError::response_body` retains the original, but `Display` truncates to 2 KiB, enough for diagnostics without logging entire HTML error pages.
- `Headers::Debug` redacts sensitive headers including `authorization`, `x-api-key`, and `cookie`.

## 5. Verification items

- [Fact] (PV-013, `verification/pv013-error-size`) Inlining the chapter's payloads makes `Error` 360 bytes. Boxing `Provider` (`ProviderError` 240 bytes), `Download` (`Url` 88 bytes), `InvalidToolInput`, `NoObjectGenerated` (`ResponseMetadata` 232 bytes), `NoSuchProvider`, and `Mcp` (104 bytes) reduces it to 56 bytes; `static_assertions::const_assert!(size_of::<Error>() <= 128)` passes.
- [Decision] Box those six payloads as `Box<...Details>` or `Box<ProviderError>`. Keep `large-error-threshold = 128` in `clippy.toml` and retain the assertion in core error tests.

## 6. Implementation record (2026-09-13)

- [Fact] Per PV-013, box `Provider`, `DownloadDetails`, `InvalidToolInputDetails`, `NoObjectGeneratedDetails`, and `NoSuchProviderDetails`. `InvalidArgument.argument` is `String`; `InvalidDataContent` adds `message`; `Mcp(BoxError)` erases its type because the core does not depend on `ferrin-mcp`. Provider cancellation maps to core cancellation; tests assert the 128-byte limit.
- [Decision] Add `Error::Stream(Box<StreamError>)` for provider error chunks in non-event consumption paths (`stream_transcribe::text_stream()` and batch result streams); `Error::ToolChoiceNotSatisfied { expected: Option<ToolName> }` ([Generation loop](07-generation-loop-and-streaming.md), section 6); and `Error::NoVideoGenerated { responses }` for all-empty video generation.
- [Fact] `ErrorKind` mapping: `Provider`/`Stream` → `Provider`; `Retry` → `Retry`; timeout/cancel retain their categories; `InvalidArgument`, `InvalidPrompt`, `MessageConversion`, `Download`, `InvalidDataContent`, `InvalidStreamPart` → `InvalidInput`; tool-related errors including unmet tool choice → `Tool`; missing object/output/image/speech/transcript/video → `Output`; `NoSuchProvider`/`NoDefaultRegistry` → `NotFound`.
- [Fact] `Error::is_retryable()` delegates provider errors, uses the last error for exhausted retries, and uses the chunk's flag for stream errors.
