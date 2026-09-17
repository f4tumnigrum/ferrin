# Core data model

**English** | [Chinese](../zh-CN/01-architecture/03-core-data-model.md)

This document defines types shared across crates. Specification types live in `ferrin-spec`; application types live in `ferrin-message`. All identifiers use English; section 8 defines serialization conventions.

## 1. JSON values

[Fact] Provider options, metadata, and tool inputs/outputs are arbitrary JSON in provider APIs. The SDK needs one JSON value type across the specification, core, and adapters.

[Decision] Use `serde_json::Value` and `serde_json::Map<String, Value>` directly, aliased as `JsonValue` and `JsonObject` in `ferrin_spec::json`. Custom JSON types would cause redundant conversion with the serde ecosystem; `serde_json` is the de facto standard.

[Decision] Complete and repaired JSON parsing reject object keys named `__proto__` and object-valued `constructor` entries containing `prototype`, at every nesting level. This matches the reference SDK's accepted-input boundary even though Rust maps have no JavaScript prototype chain (ADR 0026; reference `packages/provider-utils/src/secure-json-parse.ts`). Enforce the existing byte/depth limits in both parsing paths; ordinary `constructor` and `prototype` keys remain valid.

[Decision] Enable `serde_json`'s `preserve_order` in `ferrin-spec`. Tool-definition and object-key order affect provider prompt cache hits (OpenAI and Anthropic key caches by request prefix), and fixture snapshots need stable ordering. Document in the facade that Cargo unification enables this feature downstream.

## 2. Identifiers

[Decision] Use newtypes instead of bare `String` for these identifiers, implementing `Display`, `From<String>`, `AsRef<str>`, and `Serialize`/`Deserialize` with transparent string serialization:

| Type | Purpose |
| --- | --- |
| `ProviderId` | Provider identifier, such as `openai.responses` or `anthropic.messages` |
| `ModelId` | Model identifier |
| `ToolName` | Tool name |
| `ToolCallId` | Tool call ID |
| `ApprovalId` | Approval request ID |
| `PartId` | ID of a text, reasoning, or tool-input part in a stream |

Newtypes prevent mistakes such as passing a tool name as a tool call ID at compile time, at the cost of one construction-time conversion.

## 3. Shared types (`ferrin_spec::shared`)

### 3.1 Provider options and metadata

```rust
/// Provider-specific request options, grouped by provider key.
pub type ProviderOptions = BTreeMap<String, JsonObject>;
/// Provider-specific response metadata, grouped by provider key.
pub type ProviderMetadata = BTreeMap<String, JsonObject>;
/// Provider-side resource identifiers, e.g. `{ "openai": "file-abc123" }`.
pub type ProviderReference = BTreeMap<String, String>;
```

[Decision] All three use provider names (`openai`, `anthropic`, etc.) as keys, with JSON objects for options/metadata and strings for references. This allows options for multiple adapters in one request; each adapter reads only its own key.

### 3.2 Warnings

[Decision] `Warning` has four variants: `unsupported {feature, details?}` (ignored option), `compatibility {feature, details?}` (approximate mapping), `deprecated {setting, message}`, and `other {message}`. These cover adapter reports that should not interrupt the call.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Warning {
    Unsupported { feature: String, #[serde(default)] details: Option<String> },
    Compatibility { feature: String, #[serde(default)] details: Option<String> },
    Deprecated { setting: String, message: String },
    Other { message: String },
}
```

### 3.3 Headers

[Decision] Use `Headers`, a wrapper around `http::HeaderMap`, for request and response headers, with `merge` (later values override earlier ones, skipping `None`) and `with_user_agent_suffix`. Layered overrides, with `None` representing deletion, let callers replace defaults. `http` is shared by reqwest/hyper, avoiding extra conversion.

### 3.4 Usage

[Decision] Split usage into input counts (total, uncached, cache read, cache write) and output counts (total, text, reasoning). Every count is optional; retain the `raw` provider usage object in `raw`. Aggregate steps field by field, retaining absence rather than treating it as zero. Providers report different detail (Anthropic cache reads/writes, OpenAI reasoning tokens); distinguishing missing from zero avoids misleading billing totals.

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: InputTokens,
    pub output: OutputTokens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<JsonObject>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InputTokens {
    pub total: Option<u64>,
    pub no_cache: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputTokens {
    pub total: Option<u64>,
    pub text: Option<u64>,
    pub reasoning: Option<u64>,
}

impl Usage {
    /// Adds two usages. `None + None = None`; `Some(a) + None = Some(a)`.
    pub fn add(&self, other: &Usage) -> Usage;
}
```

### 3.5 Finish reason

[Decision] A finish reason combines a normalized enum (`stop`, `length`, `content-filter`, `tool-calls`, `error`, `other`) with the provider's `raw` value. The core uses the normalized value to decide tool execution; applications use the `raw` value for diagnosis and telemetry.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReasonKind { Stop, Length, ContentFilter, ToolCalls, Error, Other }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReason {
    pub unified: FinishReasonKind,
    pub raw: Option<String>,
}
```

## 4. Specification prompt (`ferrin_spec::language_model::prompt`)

[Decision] A prompt is an array of messages: `system` (text), `user` (text | file), `assistant` (text | file | reasoning | reasoning-file | `tool`-call | `tool`-result | `custom`; application-only `tool-approval-request` is stripped during conversion), or `tool` (`tool`-result | `tool`-approval-response). Each message and part may carry `provider_options`. This covers the three providers' message models; provider-specific forms use `provider_options` and `custom` parts.

[Decision] File `data` uses `FileData` (bytes, URL, provider reference, or inline text; four states below). `media_type` is a full IANA type or top-level type (`image`, `audio`, etc.); wildcard `*` subtypes normalize to the top-level type. `filename` is optional. A top-level type lets callers declare a category before detection so adapters can decide whether magic-byte inspection is needed.

```rust
pub type Prompt = Vec<PromptMessage>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum PromptMessage {
    System { content: String, #[serde(default)] provider_options: Option<ProviderOptions> },
    User { content: Vec<UserPromptPart>, #[serde(default)] provider_options: Option<ProviderOptions> },
    Assistant { content: Vec<AssistantPromptPart>, #[serde(default)] provider_options: Option<ProviderOptions> },
    Tool { content: Vec<ToolPromptPart>, #[serde(default)] provider_options: Option<ProviderOptions> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UserPromptPart {
    Text(TextPart),
    File(FilePart),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantPromptPart {
    Text(TextPart),
    File(FilePart),
    Reasoning(ReasoningPart),
    ReasoningFile(ReasoningFilePart),
    Custom(CustomPart),
    ToolCall(ToolCallPart),
    ToolResult(ToolResultPart),          // provider-executed results
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolPromptPart {
    ToolResult(ToolResultPart),
    ToolApprovalResponse(ToolApprovalResponsePart),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FileData {
    #[serde(rename = "data")]
    Bytes { #[serde(with = "base64_bytes")] data: Bytes },
    Url { url: Url },
    Reference { reference: ProviderReference },
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePart {
    pub data: FileData,
    pub media_type: MediaType,
    #[serde(default)] pub filename: Option<String>,
    #[serde(default)] pub provider_options: Option<ProviderOptions>,
}
```

[Decision] Since 2026-09-13, `FileData` uses four `type`-tagged states: `data` (bytes), `url`, `reference` (provider `file` `reference`), and `text` (inline `text`). The early untagged three-state draft lacked `text` and could not distinguish it from base64 strings. Prompt files and tool-result content use all four states; generated files (`file`/`reasoning-file` in result content and stream parts) use only `data`/`url`, enforced by adapter convention rather than separate types. Explicit tags remove serialization ambiguity; `text` covers inline documents supported by Anthropic and others, and references represent Files API upload IDs.

`MediaType` wraps a string with `is_full()`, `top_level()`, and `normalize()` (turning `image/*` into `image`).

[Decision] Tool-call parts carry `tool_call_id`, `tool_name`, `input` (JSON value), `provider_executed?`, and `provider_options?`. Tool-result parts carry `tool_call_id`, `tool_name`, `output`, and `provider_options?`; `output` variants are `text`, `json`, `execution-denied {reason?}`, `error-text`, `error-json`, and `content [{text} | {file(data, media_type, filename?)} | ...]`. Distinguishing success, denial, and error lets adapters set provider error flags (supported by OpenAI and Anthropic tool results).

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultOutput {
    Text { value: String },
    Json { value: JsonValue },
    ExecutionDenied { #[serde(default)] reason: Option<String> },
    ErrorText { value: String },
    ErrorJson { value: JsonValue },
    Content { value: Vec<ToolResultContentPart> },
}
```

## 5. Generated content (`ferrin_spec::language_model::content`)

[Decision] `Content` variants are `text`, `reasoning`, `reasoning-file`, `file`, `custom {kind: "<provider>.<type>"}`, `source {source_type: url | document}`, `tool-call {tool_call_id, tool_name, input: string, provider_executed?, dynamic?, provider_metadata?}`, `tool-result {tool_call_id, tool_name, result, is_error?, preliminary?, dynamic?}`, and `tool-approval-request`. Tool-call `input` remains a JSON string in the specification; the core parses and validates it. Providers may return incomplete or invalid JSON, so preserving the original allows repair or invalid-call reporting without losing `input`.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Content {
    Text { text: String, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    Reasoning { text: String, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ReasoningFile { data: FileData, media_type: MediaType, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    File { data: FileData, media_type: MediaType, #[serde(default)] filename: Option<String>, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    Custom { kind: CustomKind, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    Source(Source),
    ToolCall(ToolCall),
    ToolResult(ProviderToolResult),
    ToolApprovalRequest { approval_id: ApprovalId, tool_call_id: ToolCallId, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub tool_name: ToolName,
    /// Raw JSON text as emitted by the provider. Parsed and validated by the core.
    pub input: String,
    #[serde(default)] pub provider_executed: bool,
    #[serde(default)] pub dynamic: bool,
    #[serde(default)] pub provider_metadata: Option<ProviderMetadata>,
}
```

`CustomKind` validates the `provider.type` format.

## 6. Stream events (`ferrin_spec::language_model::stream_part`)

[Decision] `StreamPart` includes `stream-start {warnings}`, `response-metadata {id?, timestamp?, model_id?}`, `text-start/text-delta/text-end {id}`, `reasoning-start/delta/end {id}`, `tool-input-start {id, tool_name, provider_executed?, dynamic?} / tool-input-delta {id, delta} / tool-input-end {id}`, `tool-call`, `tool-result`, `tool-approval-request`, `file`, `reasoning-file`, `source`, `custom`, `finish {finish_reason, usage, provider_metadata?}`, `raw {raw_value}`, and `error {error}`. Text, reasoning, and tool input use `start`/`delta`/`end` with IDs so concurrent parts can interleave; `raw` optionally passes original provider chunks through.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum StreamPart {
    StreamStart { warnings: Vec<Warning> },
    ResponseMetadata { #[serde(default)] id: Option<String>, #[serde(default)] timestamp: Option<DateTime<Utc>>, #[serde(default)] model_id: Option<ModelId> },
    TextStart { id: PartId, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    TextDelta { id: PartId, delta: String, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    TextEnd { id: PartId, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ReasoningStart { id: PartId, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ReasoningDelta { id: PartId, delta: String, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ReasoningEnd { id: PartId, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ToolInputStart { id: ToolCallId, tool_name: ToolName, #[serde(default)] provider_executed: bool, #[serde(default)] dynamic: bool, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    ToolInputDelta { id: ToolCallId, delta: String },
    ToolInputEnd { id: ToolCallId },
    ToolCall(ToolCall),
    ToolResult(ProviderToolResult),
    ToolApprovalRequest { approval_id: ApprovalId, tool_call_id: ToolCallId },
    File { data: FileData, media_type: MediaType, #[serde(default)] filename: Option<String> },
    ReasoningFile { data: FileData, media_type: MediaType },
    Source(Source),
    Custom { kind: CustomKind, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    Finish { finish_reason: FinishReason, usage: Usage, #[serde(default)] provider_metadata: Option<ProviderMetadata> },
    Raw { raw_value: JsonValue },
    Error { error: StreamError },
}
```

[Decision] `tool-input-start` also has optional `title`; `tool-input-delta`, `tool-input-end`, `tool-approval-request`, `file`, and `reasoning-file` may carry `provider_metadata`. Metadata such as OpenAI Responses item IDs must reach applications so later steps can reference it.

`StreamError` is a serializable error carrier (message, type, status code, retryability, raw data), constructed by adapters from provider error frames.

## 7. Application messages (`ferrin_message`)

[Decision] Application `Message` differs from specification `PromptMessage` as follows:

- Content may be a string, equivalent to one text part.
- User messages accept `image` parts (bytes, base64 strings, or URLs), normalized to file parts with media type detection.
- File data supports all four `FileData` states (`{type:'data'}`, `{type:'url'}`, `{type:'reference'}`, `{type:'text'}`), plus raw bytes, base64 strings, and URLs.
- Tool messages use `ToolResultOutput` (text, JSON, denial, error text, error JSON, content arrays) and allow `tool-approval-response {approval_id, approved, reason?, provider_executed?}`. Signatures appear only on `tool-approval-request {approval_id, tool_call_id, reason?, is_automatic?, signature?}` and are recomputed from the request for validation.
- Assistant messages may contain provider-executed `tool-result` and `tool-approval-request` parts.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System(SystemMessage),
    User(UserMessage),
    Assistant(AssistantMessage),
    Tool(ToolMessage),
}

pub struct UserMessage {
    pub content: UserContent,            // Text(String) | Parts(Vec<UserPart>)
    pub provider_options: Option<ProviderOptions>,
}

#[non_exhaustive]
pub enum UserPart {
    Text(TextPart),
    Image(ImagePart),                    // convenience; normalized to File
    File(FilePart),
}

pub enum FileSource {
    Bytes(Bytes),
    Base64(String),
    Url(Url),
    Reference(ProviderReference),
    Text(String),                        // inline text document
}
```

Convenience constructors: `Message::system("...")`, `Message::user("...")`, `UserPart::image_bytes(bytes)`, `UserPart::file_url(url, "application/pdf")`.

[Fact] Implementation on 2026-09-13 (`ferrin-message`):

- Parts identical to the specification (`TextPart`, `ReasoningPart`, `CustomPart`, `ToolCallPart`, `ToolResultPart`, `ToolResultOutput`, `ToolResultContentPart`) reuse `ferrin_spec` types and are re-exported at the `ferrin_message` root. Application-only additions are `ImagePart`, `FilePart`, `ReasoningFilePart` (`data: FileSource`), `ToolApprovalRequest {approval_id, tool_call_id, reason?, is_automatic, signature?}`, and `ToolApprovalResponse {approval_id, approved, reason?, provider_executed}`.
- `FileSource` serializes with `type` tags `data` (`base64` bytes), `base64`, `url`, `reference`, `text`, and `path`. It provides I/O-free `TryFrom<FileSource> for FileData` (decodes `base64`; returns `FileSourceError::UnreadPath` for `path`) and lossless `From<FileData>`. The core reads `Path` with `tokio::fs` during conversion.
- `UserContent`/`AssistantContent` use `#[serde(untagged)]` with `Text(String) | Parts(Vec<_>)`; `Message::is_empty()` is true for empty strings and empty part lists.
- `MessagesExt`, implemented for `Vec<Message>`, provides `push_approval_response` (append to the last tool message or create one) and `pending_approval_requests`.

[Decision] Application `ToolResultOutput` does not define separate content variants for each `file`/image source (data, URL, `file` ID, provider reference). With no legacy compatibility burden, `file` plus four-state `FileData` is sufficient.

[Decision] Keep distinct application and specification message types. Conversion performs URL downloads, media type detection, provider-reference passthrough, and approval-response stripping, which depend on networking and configuration and should not occur in data constructors. Specification types remain pure, serializable data suitable for fixtures.

## 8. Serialization conventions

- Enums use `#[serde(tag = "type")]` (or `role`) with kebab-case tags for cross-language consumption.
- Field names serialize as snake_case (Rust's default).
- Binary data serializes as base64 strings with the standard alphabet and padding.
- Timestamps serialize as RFC 3339 strings.
- Omit absent optional fields with `skip_serializing_if = "Option::is_none"`.
- Mark public enums `#[non_exhaustive]` to allow specification evolution. Downstream matches need a wildcard; the coding standards explain how this relates to exhaustive matching within the workspace.
