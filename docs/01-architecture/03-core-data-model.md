# 核心数据模型

本文档定义跨 crate 共享的数据类型。规范层类型位于 `ferrin-spec`，应用侧类型位于 `ferrin-message`。所有类型均为英文标识符；序列化格式在第 8 节统一约定。

## 1. JSON 值

【事实】供应商选项、供应商元数据与工具输入输出在供应商 API 中都是任意 JSON，SDK 需要一个 JSON 值类型贯穿规范层、核心层与适配器。

【决策】Ferrin 直接使用 `serde_json::Value` 与 `serde_json::Map<String, Value>`，在 `ferrin_spec::json` 中提供类型别名 `JsonValue`、`JsonObject`。依据：自定义 JSON 类型会造成与 serde 生态的重复转换；`serde_json` 是事实标准。

【决策】`ferrin-spec` 启用 `serde_json` 的 `preserve_order` feature。依据：工具定义与对象键的顺序影响供应商侧提示缓存命中（OpenAI 与 Anthropic 的缓存以请求前缀为键），fixture 快照也依赖稳定键序。该 feature 经 Cargo 统一后对下游生效，在门面 crate 文档中说明。

## 2. 标识符

【决策】以下标识符使用 newtype 而非裸 `String`，实现 `Display`、`From<String>`、`AsRef<str>`、`Serialize`/`Deserialize`（透明序列化为字符串）：

| 类型 | 用途 |
| --- | --- |
| `ProviderId` | 供应商标识，如 `openai.responses`、`anthropic.messages` |
| `ModelId` | 模型标识 |
| `ToolName` | 工具名 |
| `ToolCallId` | 工具调用 ID |
| `ApprovalId` | 审批请求 ID |
| `PartId` | 流中文本/推理/工具输入部件的 ID |

依据：newtype 可以在编译期阻止将工具名误传为工具调用 ID 一类的错误，成本只是构造时的一次转换。

## 3. 共享类型（`ferrin_spec::shared`）

### 3.1 供应商选项与元数据

```rust
/// Provider-specific request options, grouped by provider key.
pub type ProviderOptions = BTreeMap<String, JsonObject>;
/// Provider-specific response metadata, grouped by provider key.
pub type ProviderMetadata = BTreeMap<String, JsonObject>;
/// Provider-side resource identifiers, e.g. `{ "openai": "file-abc123" }`.
pub type ProviderReference = BTreeMap<String, String>;
```

【决策】三者的键为供应商名（`openai`、`anthropic` 等），值为 JSON 对象（选项与元数据）或字符串（引用）。依据：按供应商名分组使多个适配器的选项可以共存于同一请求，适配器只读取自己的键并忽略其余。

### 3.2 警告

【决策】`Warning` 有四个变体：`unsupported {feature, details?}`（选项被忽略）、`compatibility {feature, details?}`（选项被近似映射）、`deprecated {setting, message}`、`other {message}`。依据：四类覆盖适配器需要向调用方报告而又不应中断调用的全部情形。

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

### 3.3 头部

【决策】请求头与响应头使用 `http::HeaderMap` 的封装 `Headers`，提供 `merge`（后者覆盖前者、跳过 `None` 值）与 `with_user_agent_suffix`。依据：合并时“后者覆盖前者、`None` 表示删除”让调用方可以逐层覆盖默认头；`http` crate 是 reqwest/hyper 的公共类型，避免二次转换。

### 3.4 用量

【决策】用量分为输入计数（总数、未缓存、缓存读、缓存写）与输出计数（总数、文本、推理）两组，各计数可缺失，另保留供应商原始用量对象 `raw`。核心层聚合多步用量时逐字段相加，缺失视为缺失而非 0。依据：各供应商报告的计数粒度不同（Anthropic 区分缓存读写，OpenAI 区分推理令牌），缺失与 0 必须可区分，否则聚合结果会误导计费。

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

### 3.5 完成原因

【决策】完成原因由统一枚举（`stop`、`length`、`content-filter`、`tool-calls`、`error`、`other`）与供应商原始值 `raw` 组成。依据：核心循环只依赖统一值判断是否执行工具，原始值供应用诊断与遥测。

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

## 4. 规范层 Prompt（`ferrin_spec::language_model::prompt`）

【决策】规范层 prompt 是消息数组；消息角色为 `system`（纯文本）、`user`（text | file）、`assistant`（text | file | reasoning | reasoning-file | tool-call | tool-result | custom；`tool-approval-request` 只存在于应用侧消息，转换时被剥离）、`tool`（tool-result | tool-approval-response）；每个消息与部件可携带 `provider_options`。依据：这是三家供应商消息模型的公共上界，供应商特有的部件形态通过 `provider_options` 与 `custom` 部件表达。

【决策】文件部件的 `data` 为 `FileData`（字节、URL、供应商引用或内联文本，见下文四态）；`media_type` 为完整 IANA 类型或顶级类型（`image`、`audio` 等），`*` 子类型通配符归一化为顶级类型；`filename` 可选。依据：顶级类型允许调用方在探测前先声明大类，适配器据此决定是否需要魔数探测。

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

【决策】2026-09-13 起 `FileData` 采用 `type` 标签四态：`data`（字节）、`url`、`reference`（供应商文件引用）、`text`（内联文本）；早期草案为无标签三态，缺少 `text` 且无法与 base64 字符串区分。提示词文件部件与工具结果内容使用全部四态，生成文件（结果内容与流部件的 `file`/`reasoning-file`）只使用 `data`/`url`，由适配器约定保证而不在类型上再拆分。依据：显式标签使序列化形式无歧义；`text` 是 Anthropic 等供应商支持的内联文本文档形态；供应商引用对应 Files API 上传后的标识。

`MediaType` 是对字符串的封装，提供 `is_full()`、`top_level()`、`normalize()`（把 `image/*` 归一为 `image`）。

【决策】工具调用部件携带 `tool_call_id`、`tool_name`、`input`（JSON 值）、`provider_executed?`、`provider_options?`；工具结果部件携带 `tool_call_id`、`tool_name`、`output`、`provider_options?`，其中输出变体为 `text`、`json`、`execution-denied {reason?}`、`error-text`、`error-json`、`content [{text} | {file(data, media_type, filename?)} | ...]`。依据：输出变体区分成功、拒绝与错误，适配器据此决定是否以错误形态回传给模型（OpenAI 与 Anthropic 的工具结果都支持错误标记）。

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

## 5. 生成结果内容（`ferrin_spec::language_model::content`）

【决策】生成结果内容 `Content` 的变体：`text`、`reasoning`、`reasoning-file`、`file`、`custom {kind: "<provider>.<type>"}`、`source {source_type: url | document}`、`tool-call {tool_call_id, tool_name, input: string, provider_executed?, dynamic?, provider_metadata?}`、`tool-result {tool_call_id, tool_name, result, is_error?, preliminary?, dynamic?}`、`tool-approval-request`。工具调用的 `input` 在规范层是 JSON 字符串，由核心层解析与校验。依据：供应商可能返回不完整或非法的工具参数 JSON，规范层原样保留使核心层能够修复或标记为无效调用而不丢失原文。

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

`CustomKind` 校验形如 `provider.type` 的格式。

## 6. 流事件（`ferrin_spec::language_model::stream_part`）

【决策】流事件 `StreamPart` 包含：`stream-start {warnings}`、`response-metadata {id?, timestamp?, model_id?}`、`text-start/text-delta/text-end {id}`、`reasoning-start/delta/end {id}`、`tool-input-start {id, tool_name, provider_executed?, dynamic?} / tool-input-delta {id, delta} / tool-input-end {id}`、`tool-call`、`tool-result`、`tool-approval-request`、`file`、`reasoning-file`、`source`、`custom`、`finish {finish_reason, usage, provider_metadata?}`、`raw {raw_value}`、`error {error}`。依据：文本、推理与工具输入都以 `start`/`delta`/`end` 三段式携带 `id`，使并行产生的多个部件可以交错到达；`raw` 部件按需透传供应商原始分片。

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

【决策】`tool-input-start` 另有可选 `title`；`tool-input-delta`、`tool-input-end`、`tool-approval-request`、`file`、`reasoning-file` 均可携带 `provider_metadata`。依据：供应商在这些事件上附带的元数据（如 OpenAI Responses 的项 ID）需要透传给应用，多步调用回传时才能引用。

`StreamError` 是可序列化的错误载体（消息、类型、状态码、可重试性、原始数据），由适配器从供应商流内的错误帧构造。

## 7. 应用侧消息（`ferrin_message`）

【决策】应用侧 `Message` 与规范层 `PromptMessage` 的差异：

- 内容可以是字符串（等价于单个文本部件）。
- 用户消息支持 `image` 部件（字节、base64 字符串或 URL），转换时归一为文件部件并探测媒体类型。
- 文件数据支持 `FileData` 四态：`{type:'data'}`、`{type:'url'}`、`{type:'reference'}`、`{type:'text'}`，并兼容裸字节、base64 字符串与 URL。
- 工具消息中的工具结果输出为 `ToolResultOutput`（文本、JSON、拒绝、错误文本、错误 JSON、内容数组），并允许 `tool-approval-response {approval_id, approved, reason?, provider_executed?}`（签名只出现在 `tool-approval-request {approval_id, tool_call_id, reason?, is_automatic?, signature?}` 上，校验时按请求重新计算）。
- 助手消息可包含 `tool-result`（供应商执行）与 `tool-approval-request`。

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

构造便捷函数：`Message::system("...")`、`Message::user("...")`、`UserPart::image_bytes(bytes)`、`UserPart::file_url(url, "application/pdf")`。

【事实】2026-09-13 实现（`ferrin-message`）：

- 与规范层形状完全相同的部件（`TextPart`、`ReasoningPart`、`CustomPart`、`ToolCallPart`、`ToolResultPart`、`ToolResultOutput`、`ToolResultContentPart`）直接复用 `ferrin_spec` 类型并在 `ferrin_message` 根重新导出；应用侧只新增 `ImagePart`、`FilePart`、`ReasoningFilePart`（`data: FileSource`）、`ToolApprovalRequest {approval_id, tool_call_id, reason?, is_automatic, signature?}`、`ToolApprovalResponse {approval_id, approved, reason?, provider_executed}`。
- `FileSource` 序列化为 `type` 标签：`data`（base64 字节）、`base64`、`url`、`reference`、`text`、`path`；提供无 I/O 的 `TryFrom<FileSource> for FileData`（`base64` 解码、`path` 返回 `FileSourceError::UnreadPath`）与无损的 `From<FileData>`。`Path` 变体由核心层转换阶段用 `tokio::fs` 读取。
- `UserContent`/`AssistantContent` 为 `#[serde(untagged)]` 的 `Text(String) | Parts(Vec<_>)`；`Message::is_empty()` 对空字符串与空部件列表都为真。
- `MessagesExt`（对 `Vec<Message>` 实现）提供 `push_approval_response`（追加到末尾工具消息，无则新建）与 `pending_approval_requests`。

【决策】应用侧 `ToolResultOutput` 不为文件与图像的每种来源（数据、URL、文件 ID、供应商引用）分别定义内容变体。依据：Ferrin 无历史包袱，统一用 `file` + `FileData` 四态表达。

【决策】应用侧与规范层保持两套消息类型。依据：从应用侧消息到规范层 prompt 的转换承担 URL 下载、媒体类型探测、供应商引用透传与审批响应剥离，这些步骤依赖网络与配置，不应发生在纯数据类型的构造过程中；分离后规范层类型保持纯数据、可序列化、可用于 fixture。

## 8. 序列化约定

- 所有枚举使用 `#[serde(tag = "type")]`（或 `role`），标签为 kebab-case，便于跨语言消费。
- 字段名序列化为 snake_case（Rust 默认）。
- 二进制数据序列化为 base64 字符串（标准字母表、带填充）。
- 时间戳序列化为 RFC 3339 字符串。
- 可选字段缺省时不输出（`skip_serializing_if = "Option::is_none"`）。
- 公共枚举标记 `#[non_exhaustive]`，为规范演进保留空间；下游 `match` 需要通配分支，与工作区内部“穷尽匹配”约定的关系在编码规范中说明。
