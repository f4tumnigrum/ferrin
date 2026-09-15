# MCP 集成

[English](../../01-architecture/15-mcp.md) | **简体中文**

`ferrin-mcp` 提供 Model Context Protocol 客户端，把远端工具接入 Ferrin 工具集。

## 1. 能力范围

【决策】`ferrin-mcp` 覆盖的 MCP 客户端能力（协议依据：MCP 规范 2025-11-25 与 2026-07-28）：

- 客户端配置：传输、客户端名称、能力声明、工具调用重试次数。传输配置支持 Streamable HTTP 与旧版 SSE（URL、请求头、OAuth 提供者、重定向策略默认拒绝、初始会话 ID 与协议版本、会话变更与过期通知、关闭时终止会话），或自定义 `McpTransport` 实现（启动、发送、关闭、事件流、协议版本）。
- 协议版本协商：支持最新协议与旧版列表；声明支持版本探测的传输先探测无状态协议（超时默认 1000 ms，错误码 `-32020..-32022` 表示现代协议）。
- `tools(ToolsOptions)` 列出工具并返回工具集：提供显式 schema 时构造类型化工具，否则构造动态工具；工具的执行函数调用 `tools/call`，结果 `CallToolResult` 的 `content` 与 `isError` 转换为工具输出。
- 工具调用重试：默认 0 次；可重试判定为状态码 408/409/429/≥500 或连接类错误；带 JSON-RPC 错误码的错误不重试。
- 请求超时与总超时取最小值；支持取消令牌。
- 还支持资源列举与读取、资源模板、提示列举与获取、补全、`discover`、诱导（elicitation）请求处理、MCP Apps（应用工具拆分、应用资源读取、资源指纹与漂移检测）、`x-mcp-header` 参数到请求头的绑定、OAuth（`auth`、`OAuthClientProvider`、`McpError::Unauthorized`）。

## 2. 架构

```
ferrin-mcp
  ├── transport/
  │     ├── mod.rs           McpTransport trait, TransportConfig, SendOptions/CloseOptions/TransportEvent
  │     ├── common.rs        shared HTTP helpers (headers, bounded bodies, SSE pump, OAuth hook)
  │     ├── headers.rs       x-mcp-header binding → Mcp-Param-*
  │     ├── http.rs / http_config.rs   Streamable HTTP (POST + SSE, legacy session, GET inbound stream)
  │     ├── sse.rs           legacy: GET SSE + POST endpoint
  │     └── stdio.rs         child process (feature "stdio")
  ├── protocol/
  │     ├── json_rpc.rs      JSON-RPC 2.0 messages and error codes
  │     ├── types.rs         MCP method params/results (serde, forward-compatible `extra`)
  │     └── versions.rs      protocol version constants, ProtocolEra, _meta keys
  ├── client/
  │     ├── mod.rs           McpClient, McpClientConfig, ElicitationHandler, dispatch loop
  │     ├── request.rs       _meta injection, timeouts, input_required rounds, negotiation
  │     ├── methods.rs       tools/resources/prompts/completion/logging methods
  │     └── server_requests.rs   ping / elicitation/create handling
  ├── tools.rs             McpClient::tools() → ToolSet (dynamic or typed), mcp_to_model_output
  ├── apps.rs              MCP Apps: split tools, read app resource, fingerprint & drift
  ├── oauth/               OAuthClientProvider trait, discovery, PKCE flow, token refresh (feature "oauth")
  └── error.rs             McpError, TransportFailure
```

【决策】（2026-09-14）模块布局按上图实现，与最初规划相比：`headers.rs` 归入 `transport/`（绑定只在 HTTP 传输生效）；`resources.rs`/`prompts.rs`/`completion.rs` 合并为 `client/methods.rs`（每个方法只有参数构造与结果反序列化，单独成文件没有内容）；`client.rs` 拆为四个子模块以满足 500 行目标。

### 2.1 传输 trait

```rust
pub trait McpTransport: Send + Sync + 'static {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>>;
    fn send(&self, message: JsonRpcMessage, options: SendOptions) -> BoxFuture<'_, Result<(), McpError>>;
    fn incoming(&self) -> BoxStream<'static, TransportEvent>;   // Message | Closed | Error
    fn close(&self, options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>>;
    fn protocol_version(&self) -> Option<String>;
    fn set_protocol_version(&self, version: Option<&str>);      // None: cleared before a legacy `initialize`
    fn capabilities(&self) -> TransportCapabilities;            // supports_protocol_version_discovery, supports_tool_parameter_headers
}
```

【决策】协议版本以 `String` 而非新类型表示：服务端在 `server/discover` 与 `-32022` 的 `data.supported` 中可以宣告本 crate 未知的未来版本，客户端只需比较字符串并按 `SUPPORTED_PROTOCOL_VERSIONS` 的顺序挑选公共版本；`ProtocolEra::of_version` 负责“现代/旧版”的二分。`incoming()` 只能被消费一次（再次调用得到空流），由 `McpClient` 的分发任务独占。

【决策】以事件流 `incoming()` 取代消息、关闭、错误三个回调。依据：Rust 中回调需要 `Mutex<Option<Box<dyn Fn>>>` 一类的可变共享状态；单一事件流更符合所有权模型，并让客户端用 `select!` 统一处理消息与关闭。

### 2.2 协议实现来源

【决策】`ferrin-mcp` 自行实现 JSON-RPC 与 MCP 消息类型，不依赖 `rmcp`。依据：

- 需要的协议子集（initialize、tools、resources、prompts、completion、elicitation、logging 通知）有限，且必须与 Ferrin 的 `Schema`、`ToolResultOutput`、审批与指纹机制紧密耦合。
- 传输层行为（重定向策略默认 `error`、会话过期回调、恢复令牌、`x-mcp-header` 绑定、协议版本探测）由 MCP 规范与两代协议的差异决定，需要完全控制。
- 避免把第三方 SDK 的类型暴露在公共 API 中，减少版本联动。

### 2.2.1 协议版本与两代传输语义

【事实】（PV-017）MCP 规范当前版本为 2026-07-28，前一版本为 2025-11-25（规范站点 `modelcontextprotocol.io/specification/`，2026-09-13 访问）；`protocol/versions.rs` 以此定义最新版本、旧版本与支持列表常量。

【事实】MCP 规范 2026-07-28（`basic/transports/streamable-http`、`basic/versioning`）相对 2025-11-25 的变化：

- 无 `initialize` 握手；每个请求在 `params._meta` 中携带 `io.modelcontextprotocol/protocolVersion`、`io.modelcontextprotocol/clientInfo`、`io.modelcontextprotocol/clientCapabilities`，服务端逐请求接受或以 `UnsupportedProtocolVersionError`（`-32022`，`data.supported` 列出支持版本）拒绝；服务端必须实现 `server/discover`。
- Streamable HTTP 只保留单一端点的 POST；客户端必须发送 `Accept: application/json, text/event-stream`、`MCP-Protocol-Version`、`Mcp-Method`，以及 `tools/call`/`resources/read`/`prompts/get` 的 `Mcp-Name`；被 `x-mcp-header` 标注的工具参数镜像为 `Mcp-Param-{name}`；非 ASCII 值以 `=?base64?...?=` 编码；头与体不一致返回 400 + `HeaderMismatch`（`-32020`）；未知方法返回 404 + `-32601`。
- 响应为单个 JSON 或仅限该请求的 SSE 流；移除 GET 长连接、`Mcp-Session-Id` 会话、`Last-Event-ID` 恢复与服务端发起的请求；采样、诱导、roots 改为 MRTR（结果中的 `InputRequiredResult.inputRequests`，客户端携带 `inputResponses` 重试原请求）；列表变更通知通过 `subscriptions/listen` 请求的响应流投递；取消 = 关闭响应流。
- 版本探测：先发现代请求，收到 400 时检查响应体，能识别的现代 JSON-RPC 错误说明对方是现代服务端（按 `supported` 重试），否则回退到 `initialize`；服务端所属“代”按 origin 缓存。

【决策】版本探测与两代传输行为：协议版本探测默认开启且传输声明支持（HTTP 传输为真）时，先以 1000 ms 超时探测 `server/discover`；错误码属于 `[-32020, -32021, -32022]` 时判定为现代服务端并直接返回错误，其他失败回退到 `initialize`（`2025-11-25`）+ `notifications/initialized`；探测结果的 `supportedVersions` 须包含所请求的版本。传输在旧协议下：请求头 `mcp-protocol-version` 为协商版本、`mcp-session-id` 仅在旧协议携带、关闭时对端点 `DELETE`（可配置）、GET 开启入站 SSE（405 视为不支持）、`last-event-id` 携带恢复令牌、带会话的 404 触发会话过期通知；新协议下发送 `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*`。重定向模式默认拒绝。依据：两代协议的会话与恢复语义不同，客户端必须在探测结果确定后才选择传输行为。

【决策】`ferrin-mcp` 实现双代客户端：默认先探测 `server/discover`（`McpClientConfig::protocol_discovery`，默认开，超时 `discovery_timeout` 默认 1 s），成功且公共版本为现代版本时以 2026-07-28 无会话模式运行（不再发送 `initialize`）；探测失败（非 `-32020..-32022` 错误、超时、传输不支持探测或 `protocol_discovery` 关闭）或公共版本为旧版时回退到 `initialize`（请求 2025-11-25，接受服务端返回的任一受支持版本）+ `notifications/initialized`，并启用会话、GET 入站流、`last-event-id` 恢复与 `DELETE` 终止。两代共享同一 `McpTransport` trait；代际状态是 `McpClient` 的字段，**不**按 `origin` 跨客户端缓存（2026-09-14 修订：每个客户端对应一条连接，跨客户端缓存需要全局注册表，而代价只是每次连接一次 1 s 上限的探测）。诱导处理器 `ElicitationHandler` 同时服务旧协议的 `elicitation/create` 服务端请求与新协议的 MRTR 输入请求（客户端在收到 `resultType: "input_required"` 后按 2.2.2 节调用处理器并携带 `inputResponses` 重试原请求）。

### 2.2.2 MRTR（多轮往返请求）的结果结构

【事实】（PV-030，2026-09-14 关闭）MCP 规范仓库 `schema/2026-07-28/schema.ts`（`https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/2026-07-28/schema.ts`）定义：

- `Result { _meta?, resultType: ResultType }`，`ResultType = "complete" | "input_required"`；`InputRequiredResult extends Result { inputRequests?: InputRequests; requestState?: string }`（两者至少一个存在）。
- `InputRequests = { [key: string]: InputRequest }`，`InputRequest = CreateMessageRequest | ListRootsRequest | ElicitRequest`，即带 `method` 与 `params` 的 JSON-RPC 请求对象；`InputResponses = { [key: string]: InputResponse }`，`InputResponse = CreateMessageResult | ListRootsResult | ElicitResult`。
- `InputResponseRequestParams extends RequestParams { inputResponses?: InputResponses; requestState?: string }`：客户端以原方法、原参数加上这两个字段重试请求（新的 JSON-RPC id）。
- `RequestMetaObject { progressToken?, "io.modelcontextprotocol/protocolVersion": string, "io.modelcontextprotocol/clientInfo"?: Implementation, "io.modelcontextprotocol/clientCapabilities": ClientCapabilities }`；`ResultMetaObject { "io.modelcontextprotocol/serverInfo"?: Implementation }`；`DiscoverResult { supportedVersions: string[], capabilities: ServerCapabilities, instructions? }`；错误码 `-32020 HeaderMismatch`、`-32021 MissingRequiredClientCapability`、`-32022 UnsupportedProtocolVersion { data: { supported, requested } }`。

【决策】`ferrin-mcp` 的实现：现代代下结果缺少 `resultType` 视为协议错误（`McpError::Protocol`）；`input_required` 时对 `inputRequests` 的每个条目调用 `ElicitationHandler`（仅接受 `method == "elicitation/create"`；`sampling/createMessage` 与 `roots/list` 输入请求返回 `McpError::Protocol`，本 crate 不提供采样与 roots），把结果按键写入 `inputResponses`，连同服务端返回的 `requestState` 重试原请求，轮数上限 `McpClientConfig::max_input_rounds`（默认 8）；未注册处理器时返回 `McpError::Elicitation`。服务端信息取自 `DiscoverResult._meta["io.modelcontextprotocol/serverInfo"]`。

### 2.3 客户端

```rust
pub struct McpClient { /* Arc<inner>; Clone */ }

impl McpClient {
    pub async fn connect(config: McpClientConfig) -> Result<Self, McpError>;
    pub async fn tools(&self, options: ToolsOptions) -> Result<ToolSet, McpError>;
    pub fn tools_from_definitions(&self, definitions: Vec<McpTool>, options: &ToolsOptions) -> Result<ToolSet, McpError>;
    pub async fn list_tools(&self, cursor: Option<&str>, options: RequestOptions) -> Result<ListToolsResult, McpError>;
    pub async fn list_all_tools(&self, options: RequestOptions) -> Result<Vec<McpTool>, McpError>;
    pub async fn call_tool(&self, name: &str, arguments: Option<JsonObject>, options: RequestOptions) -> Result<CallToolResult, McpError>;
    pub async fn list_resources(&self, cursor: Option<&str>, options: RequestOptions) -> Result<ListResourcesResult, McpError>;
    pub async fn list_resource_templates(&self, cursor: Option<&str>, options: RequestOptions) -> Result<ListResourceTemplatesResult, McpError>;
    pub async fn read_resource(&self, uri: &str, options: RequestOptions) -> Result<ReadResourceResult, McpError>;
    pub async fn list_prompts(&self, cursor: Option<&str>, options: RequestOptions) -> Result<ListPromptsResult, McpError>;
    pub async fn get_prompt(&self, name: &str, arguments: Option<BTreeMap<String, String>>, options: RequestOptions) -> Result<GetPromptResult, McpError>;
    pub async fn complete(&self, params: CompleteParams, options: RequestOptions) -> Result<CompleteResult, McpError>;
    pub async fn ping(&self, options: RequestOptions) -> Result<(), McpError>;
    pub async fn set_logging_level(&self, level: &str, options: RequestOptions) -> Result<(), McpError>;
    pub async fn request(&self, method: &str, params: Option<JsonObject>, options: RequestOptions) -> Result<JsonObject, McpError>;
    pub async fn notify(&self, method: &str, params: Option<JsonObject>) -> Result<(), McpError>;
    pub fn on_elicitation(&self, handler: Arc<dyn ElicitationHandler>);
    pub fn server_capabilities(&self) -> Option<ServerCapabilities>;
    pub fn server_info(&self) -> Option<Implementation>;
    pub fn instructions(&self) -> Option<String>;
    pub fn protocol_version(&self) -> Option<String>;
    pub fn protocol_era(&self) -> ProtocolEra;
    pub async fn close(&self) -> Result<(), McpError>;
}

#[non_exhaustive]
pub struct McpClientConfig {                     // McpClientConfig::new(transport) + builder methods
    pub transport: TransportConfig,              // Http(HttpTransportConfig) | Sse(SseTransportConfig) | Stdio(StdioConfig) | Custom(SharedMcpTransport)
    pub name: String,                            // clientInfo.name, default "ferrin-mcp-client"
    pub version: String,                         // clientInfo.version, default crate version
    pub title: Option<String>,
    pub capabilities: ClientCapabilities,
    pub max_tool_call_retries: u32,              // default 0
    pub default_request_timeout: Option<Duration>,
    pub protocol_discovery: bool,                // default true
    pub discovery_timeout: Duration,             // default 1 s
    pub initialization_timeout: Option<Duration>,
    pub max_input_rounds: u32,                   // default 8
    pub on_uncaught_error: Option<UncaughtErrorHook>,
    pub on_notification: Option<NotificationHook>,
    pub elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
}

pub struct RequestOptions { pub timeout: Option<Duration>, pub max_total_timeout: Option<Duration>, pub cancellation: Option<CancellationToken>, pub headers: Headers }
```

【决策】每个方法显式接收 `RequestOptions`（超时、总超时、取消令牌、请求级头），超时取 `timeout.or(default_request_timeout)` 与 `max_total_timeout` 的最小值；超时或取消时客户端撤销挂起项并发送 `notifications/cancelled`。`server_capabilities()` 等访问器返回克隆的 `Option`（连接期间状态在 `Mutex` 内），`close(&self)` 可在任一克隆上调用并使所有挂起请求以 `McpError::Closed` 失败。

### 2.4 工具桥接

【决策】MCP 工具输出转换：`CallToolResult.content` 为 `text`/`image`/`audio`/`resource` 项数组（MCP 规范）；结果作为工具输出，`isError` 为真时作为错误输出；`to_model_output` 把内容项映射为 `content` 类型的模型输出。依据：`content` 输出保留图像与音频项，供支持多模态工具结果的供应商直接消费。

```rust
#[non_exhaustive]
pub struct ToolsOptions {
    pub schemas: ToolSchemas,                  // Automatic (dynamic tools) | Explicit(HashMap<String, ToolSchemaPair { input, output: Option<Schema> }>)
    pub request_timeout: Option<Duration>,
    pub name_prefix: Option<String>,           // avoid collisions across servers
}
```

【决策】生成的工具为 `ToolKind::Dynamic`（自动模式：`inputSchema` 补齐 `properties: {}` 并加 `additionalProperties: false`，由 `Schema::from_json_schema` 校验）或 `ToolKind::Function`（显式 schema；只包含列出的工具；给出 `output` 时校验 `structuredContent`，缺失则解析第一个文本项为 JSON，校验失败为 `ToolError::Message`）。`isError` 为真时执行器返回 `ToolError::Json(CallToolResult)`，由核心层按错误输出处理；`mcp_to_model_output` 把 `content` 数组映射为 `ToolResultOutput::Content`（`text` → 文本，`image`/`audio` → base64 解码后的文件部件，默认媒体类型 `image/png`/`audio/wav`，其余项以 JSON 文本呈现），非数组输出走 `ToolResultOutput::Json`。工具 `metadata` 为 `{clientName, toolName, title?, annotations?, app?, meta?}`。`x-mcp-header` 绑定仅在现代代且传输声明 `supports_tool_parameter_headers` 时收集；绑定非法的工具被丢弃并经 `on_uncaught_error` 报告。

### 2.5 OAuth

【决策】OAuth 模块实现授权服务器元数据发现（RFC 8414、RFC 9728）、动态客户端注册（RFC 7591）、PKCE 授权码流程（RFC 7636）与令牌刷新；`OAuthClientProvider` 由应用实现存储与重定向处理；`McpError::Unauthorized` 表示需要用户完成授权。依据：MCP 授权规范以这些 RFC 为基础，存储与浏览器重定向只有应用才能完成。

```rust
pub trait OAuthClientProvider: Send + Sync {
    fn redirect_url(&self) -> Option<Url>;
    fn client_metadata(&self) -> OAuthClientMetadata;
    fn client_information(&self) -> BoxFuture<'_, Result<Option<OAuthClientInformation>, McpError>>;
    fn save_client_information(&self, info: OAuthClientInformation) -> BoxFuture<'_, Result<(), McpError>>;   // default: rejects dynamic registration
    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>, McpError>>;
    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<(), McpError>>;
    fn redirect_to_authorization(&self, url: Url) -> BoxFuture<'_, Result<(), McpError>>;
    fn save_code_verifier(&self, verifier: String) -> BoxFuture<'_, Result<(), McpError>>;
    fn code_verifier(&self) -> BoxFuture<'_, Result<Option<String>, McpError>>;
    fn state(&self) -> BoxFuture<'_, Result<Option<String>, McpError>>;                        // default None
    fn invalidate_credentials(&self, scope: InvalidateScope) -> BoxFuture<'_, Result<(), McpError>>;   // default no-op
}

pub async fn auth(provider: &dyn OAuthClientProvider, http: &dyn HttpTransport, options: AuthOptions) -> Result<AuthResult, McpError>;  // Authorized | Redirect
```

【决策】方法返回 `Result`（存储可能失败）；`OAuthTokens`/`OAuthClientInformation` 的密钥字段为 `secrecy::SecretString`，不派生 `Serialize`，持久化须显式调用 `expose_to_json()`。`auth` 的流程：受保护资源元数据（`/.well-known/oauth-protected-resource[path]`，再回退根路径）→ 授权服务器元数据（RFC 8414 路径感知顺序，再 OpenID 配置；OpenID 文档必须宣告 `S256`）→ 无客户端信息时动态注册 → 有授权码则交换、有刷新令牌则刷新（协议错误直接返回，`server_error`/网络错误回退到新授权）→ 否则生成 PKCE `S256` 并重定向；`invalid_client`/`unauthorized_client` 使全部凭据失效后重试一次，`invalid_grant` 使令牌失效后重试一次。授权服务器的所有端点经 `AuthOptions::url_policy`（HTTP 传输传入自身的 `UrlPolicy`）校验，阻断指向内网的元数据。HTTP 传输对 `401` 只运行一次 `auth`（并发的 `401` 等待进行中的流程后按已存令牌重试）；结果为 `Redirect` 时返回 `McpError::Unauthorized`。

## 3. 与核心层的关系

- `McpClient::tools()` 返回普通 `ToolSet`，可与本地工具合并后传给 `generate_text`。
- 审批、超时、遥测由核心层按工具统一处理；MCP 工具不例外。
- `ferrin_mcp::fingerprint_app_resource` / `detect_app_resource_drift` 与 `ferrin_tool::fingerprint` 配合用于跨请求一致性检查。

## 4. 示例

```rust
use ferrin_mcp::{McpClient, McpClientConfig, ToolsOptions};
use ferrin_mcp::transport::{HttpTransportConfig, TransportConfig};
use ferrin_spec::Headers;

let url = url::Url::parse("https://mcp.example.com/mcp")?;
let transport = HttpTransportConfig::new(url).headers(Headers::new().with("x-tenant", "acme"));
let mcp = McpClient::connect(
    McpClientConfig::new(TransportConfig::Http(transport)).name("ferrin-demo"),
)
.await?;

let tools = mcp.tools(ToolsOptions::default()).await?.merge(local_tools())?;

let result = ferrin::generate_text(&model)
    .prompt("List open incidents and summarize the most severe one.")
    .tools(tools)
    .stop_when(step_count(6))
    .await?;

mcp.close().await?;
```

## 5. 待验证

- 【事实】（PV-018，已关闭）stdio 传输在 Windows 上的子进程管道与信号处理由 CI 的 `windows-2025` 作业验证：2026-09-14 首次完整 CI 运行（`ci.yml` run 34797869442）中 `ferrin-mcp` 的 stdio 测试 `missing_commands_fail_to_start`、`negotiates_lists_and_calls_tools_over_stdio`、`server_requests_are_answered_over_stdio`（以 `python` 启动的 fixture 服务器）全部通过。【决策】实现基线：`tokio::process::Command` + `kill_on_drop(true)`、stdin 由单一写入任务串行写出整帧（2026-09-14 依 [ADR 0015](../04-decisions/2026-09-14-0015-mcp-stdio-frame-writer.md) 由“持锁写入”修订），Windows 下通过 `CommandExt::creation_flags` 抑制控制台窗口并拒绝命令与参数中的换行。
- 【事实】（PV-019）MCP 规范的诱导请求参数为 `{ message: string, requestedSchema: object }`，结果为 `{ action: 'accept' | 'decline' | 'cancel', content?: object }`。【决策】Ferrin 对 `requestedSchema` 不做结构校验， `ElicitationRequest { message: String, requested_schema: JsonValue }` 与 `ElicitResult { action: ElicitAction, content: Option<JsonObject> }` 字段一一对应。
- 【事实】（PV-030，2026-09-14 关闭）2026-07-28 规范中 `InputRequiredResult`、`inputRequests`、`inputResponses` 的字段定义已从规范仓库的 `schema/2026-07-28/schema.ts` 固定，见 2.2.2 节；`ferrin-mcp` 按此实现并以 mock 传输测试（`tests/suite/client_modern.rs` 的 `input_required_*` 用例）。

## 6. 实现记录（2026-09-14，ferrin-mcp）

- 【事实】模块布局见第 2 节的树；最大文件 `protocol/types.rs` 630 行，其余均在 600 行以下。依赖：`ferrin-spec`、`ferrin-schema`、`ferrin-tool`、`ferrin-provider-util`（`reqwest` 特性）、`tokio`（`rt`、`sync`、`time`、`macros`；`stdio` 特性追加 `process`、`io-util`）、`tokio-util`、`futures-util`、`serde`/`serde_json`、`thiserror`、`bytes`、`http`、`url`、`sha2`、`base64`、`rand`、`secrecy`、`tracing`；未用到 `ferrin-message`。特性 `stdio`、`oauth` 默认开启，`cargo hack --each-feature` 通过。
- 【事实】测试 72 个（`--all-features`；不带特性 59 个）：`json_rpc`、`headers`、`client_modern`（探测、`_meta`、`resultType`、MRTR、能力断言、重试、超时取消、服务端 `ping`/`elicitation/create`、未捕获错误与关闭）、`client_legacy`（回退、禁用探测、`-32022` 中止、不支持的版本）、`resources_prompts`、`tools`、`apps`、`http_transport`（JSON/SSE 响应、会话头、202 与入站流、405、错误体、404/401/500/内容类型、重定向两种模式、现代头、`DELETE` 终止、URL 策略）、`sse_transport`、`stdio`（Python 测试服务器 `tests/fixtures/stdio/echo_server.py`：协商、工具调用、环境变量透传、服务端请求）、`oauth`（`WWW-Authenticate` 解析、发现顺序、PKCE 含 RFC 7636 向量、授权 URL、资源选择、完整流程含注册/交换/刷新/`invalid_grant` 重试、`client_secret_basic`、OpenID 缺 `S256`、传输的 `401` 刷新与未授权）。请求体快照 2 个（`tests/suite/snapshots/`）。客户端逻辑以进程内 mock 传输（`tests/suite/common.rs` 的 `MockTransport`）测试，因为 `FixtureServer` 只能回放固定响应而 JSON-RPC 响应必须回显请求 id。
- 【事实】为测试长连接给 `ferrin-testing` 增加 `Fixture::hold_open()`（事件流在最后一个事件后保持打开直到连接或服务器关闭）。
- 【决策】安全：`HttpTransportConfig::url_policy`/`SseTransportConfig::url_policy` 默认 `UrlPolicy::new()`（仅 HTTPS、拒绝内网），`start()` 时校验端点并固定解析地址（DNS pinning），之后每个请求携带 `pinned_addresses`；本地开发需显式 `.url_policy(UrlPolicy::new().allow_http().allow_private_networks())`。响应体上限 `max_response_bytes` 默认 16 MiB，单个 SSE 事件上限 `max_event_bytes` 默认 16 MiB；OAuth 响应体上限 1 MiB。`Debug` 输出对头部脱敏、不打印会话 id 与令牌。
- 【决策】错误：`McpError::Transport(Box<TransportFailure>)` 装箱以满足 128 字节的错误大小上限；`is_retryable_tool_call()` 对 HTTP 408/409/429/5xx、连接级 `TransportError::is_retryable` 与 `Io` 为真，JSON-RPC 错误恒为假。
- 【决策】Streamable HTTP：`3xx` 默认报错（`RedirectMode::Error`），`Follow` 只跟随同源 `307`/`308` 且受策略的 `max_redirects` 限制；旧版入站 GET 流断开后按 `ReconnectionOptions`（初始 1000 ms、系数 1.5、上限 30 s、最多 2 次）重连并携带 `last-event-id`，`405` 视为服务端不提供入站流；`close()` 在旧版且有会话时 `DELETE`（`terminate_session_on_close` 默认真），现代代不发送。
- 【决策】legacy SSE 传输：`start()` 等待首个 `endpoint` 事件并要求与流同源；流结束视为传输关闭（先投递 `Error` 再 `Closed`），之后 `send()` 返回 `McpError::Closed`。
- 【决策】stdio：`env_clear()` 后仅透传 `DEFAULT_INHERITED_ENV_VARS`（Unix：`HOME`、`LOGNAME`、`PATH`、`SHELL`、`TERM`、`USER`；Windows 见常量）中不以 `()` 开头的值，再叠加 `StdioConfig::env`；stderr 默认继承（`StdioStderr::Null` 可丢弃）；`close()` 先 `start_kill()` 再等待退出。写入路径见 [ADR 0015](../04-decisions/2026-09-14-0015-mcp-stdio-frame-writer.md)。
- 【决策】服务端请求：`ping` 回 `{}`；`elicitation/create` 调用处理器（无处理器 `-32601`，参数非法 `-32602`，处理器失败 `-32603`）；其余方法（含 `sampling/createMessage`、`roots/list`）回 `-32601`。服务端通知经 `on_notification` 钩子投递，无钩子时以 `tracing::debug!` 记录。
- 【决策】（2026-09-14，真实凭据验证后新增）由服务器 schema 自动构建的 MCP 工具以 `strict = false` 交给供应商（`ToolBuilder::strict(false)`）；`ToolsOptions::explicit` 提供的类型化 schema 不改动。依据：OpenAI Responses API 的函数工具默认严格校验（要求 `required` 列出全部属性且 `additionalProperties: false`），`@modelcontextprotocol/server-everything` 等服务器的 schema 普遍不满足，未标记时整次调用被拒绝（`Invalid schema for function ... 'required' is required to be supplied`）。
- 【决策】范围外：`subscriptions/listen`（现代代的列表变更通知）、除 `last-event-id` 外的恢复令牌（`SendOptions` 无 `resumption_token`）、采样与 roots、OAuth 的 `client_secret_jwt`/`private_key_jwt`。

【决策】2026-09-15 客户端任务由公开客户端句柄持有，与分发和服务端请求 future 共享的状态分离，最后一个句柄被丢弃即中止分发器及其处理器。内置传输的析构函数取消流并中止所属任务，打破任务与状态的循环引用；显式 close 仍负责协议会话终止。启动失败会关闭传输（回归覆盖：`crates/ferrin-mcp/tests/suite/client_lifecycle.rs`）。

【决策】2026-09-15 同一有效截止时间覆盖传输发送、响应接收和全部 `input_required` 轮次及处理器。请求取消也中断发送；丢弃请求会清除待处理登记并取消其私有传输令牌。超时与取消通知通过公开客户端句柄持有的任务尽力发送，独立限制在一秒内；弱引用避免循环，最后一个句柄被丢弃时中止清理，避免通知发送阻塞原始错误返回（回归覆盖：`crates/ferrin-mcp/tests/suite/client_deadlines.rs`，使用暂停的 Tokio 时钟）。 启动或初始化失败后的传输清理同样独立限制在一秒内，即使自定义 close 或 HTTP 会话 DELETE 挂起也保留原始失败；协议清理等待前先停止本地待处理工作。

【决策】2026-09-15 请求级 HTTP SSE 读取任务同时监听 `SendOptions.cancellation` 和传输取消。客户端请求结束时取消私有令牌，即使传输保持打开也会释放响应体（回归覆盖：`crates/ferrin-mcp/tests/suite/http_stream_lifecycle.rs`，使用响应体析构通知）。

【决策】2026-09-15 `TransportEvent::RequestError { id, error }` 将响应流失败归属到具体待处理请求。HTTP 请求 SSE 在收到匹配的 JSON-RPC 响应或错误后停止；无匹配响应的 EOF、无效消息和响应体或解码错误会使该请求失败，即使未配置超时。通知和其他请求 ID 不算完成（来源：`crates/ferrin-mcp/src/transport/http_stream.rs`）。
