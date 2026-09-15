# MCP integration

**English** | [Chinese](../zh-CN/01-architecture/15-mcp.md)

`ferrin-mcp` provides a Model Context Protocol client that exposes remote tools as Ferrin tool sets.

## 1. Scope

[Decision] Client capabilities, based on MCP specifications 2025-11-25 and 2026-07-28:

- Configure transport, client name, capabilities, and tool retries. Streamable HTTP and legacy SSE accept URLs, headers, OAuth providers, redirect policy (deny by default), initial session ID/version, session-change/expiry notifications, and termination on close. Custom `McpTransport` implementations provide start/send/close, incoming events, and protocol version.
- Negotiate latest and supported older versions. Discovery-capable transports first probe stateless protocols, with a 1000 ms default timeout; errors `-32020..-32022` identify modern servers.
- `tools(ToolsOptions)` lists tools and returns a set, typed when explicit schemas are supplied and dynamic otherwise. Executors call `tools/call`; `CallToolResult.content` and `isError` map to tool output.
- Tool retries default to zero. HTTP 408/409/429/≥500 and connection failures may retry; JSON-RPC errors never retry.
- Use the smaller request/total timeout and support cancellation.
- Also support resource listing/reading/templates, prompt listing/getting, completion, `discover`, elicitation handlers, MCP Apps tool splitting/resource reading/fingerprints/drift, `x-mcp-header` parameter binding, and OAuth (`auth`, `OAuthClientProvider`, `McpError::Unauthorized`).

## 2. Architecture

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

[Decision] The 2026-09-14 layout follows the tree above. Move `headers.rs` into `transport/` because binding applies only to HTTP; combine resources/prompts/completion in `client/methods.rs` because each only builds parameters and deserializes; split the client into four modules to meet the 500-line target.

### 2.1 Transport trait

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

[Decision] Use `String` for protocol versions: servers may announce unknown future versions through discovery or `-32022` `data.supported`. Select common versions in `SUPPORTED_PROTOCOL_VERSIONS` order; `ProtocolEra::of_version` distinguishes modern/legacy. `incoming()` is single-use, returning an empty stream on subsequent calls; the client dispatcher owns it.

[Decision] Replace message/close/error callbacks with one incoming event stream. This avoids mutable callback storage such as `Mutex<Option<Box<dyn Fn>>>` and lets `select!` handle messages and closure under Rust ownership.

### 2.2 Protocol implementation source

[Decision] Implement JSON-RPC/MCP types directly, without `rmcp`:

- The needed subset (initialize, tools, resources, prompts, completion, elicitation, logging notifications) is limited and tightly integrated with Ferrin schemas, outputs, approvals, and fingerprints.
- Redirect rejection, session expiry, resumption, header binding, and discovery require full control over two protocol generations.
- Avoid third-party SDK types in the public API and their version coupling.

### 2.2.1 Versions and transport generations

[Fact] (PV-017) Latest MCP specification is 2026-07-28, preceded by 2025-11-25 (`modelcontextprotocol.io/specification/`, accessed 2026-09-13). `protocol/versions.rs` defines version constants accordingly.

[Fact] Changes in 2026-07-28 (`basic/transports/streamable-http`, `basic/versioning`) relative to 2025-11-25:

- No `initialize` handshake. Every request carries protocol version, client info, and capabilities in `params._meta` under `io.modelcontextprotocol/*`. Servers accept each request or reject with `UnsupportedProtocolVersionError` (`-32022`, supported versions in `data.supported`); `server/discover` is required.
- Streamable HTTP uses POST at one endpoint. Require Accept for JSON/SSE, `MCP-Protocol-Version`, `Mcp-Method`, and `Mcp-Name` for `tools/call`, `resources/read`, and `prompts/get`. Mirror annotated arguments as `Mcp-Param-{name}`, encoding non-ASCII as `=?base64?...?=`. Header/body mismatch is HTTP 400 with `HeaderMismatch` (`-32020`); unknown methods are 404 with `-32601`.
- Responses are one JSON object or request-scoped SSE. Remove long-lived GET, sessions, `Last-Event-ID` resumption, and server-initiated requests. Sampling, elicitation, and roots use MRTR `InputRequiredResult.inputRequests`, retried with `inputResponses`. List-change notifications use `subscriptions/listen`; cancellation closes the response stream.
- Probe with a modern request; recognizable modern JSON-RPC errors in HTTP 400 identify modern servers and allow retry using `supported` versions, otherwise fall back to `initialize`. The specification describes caching server generation by origin.

[Decision] When discovery is enabled and supported (HTTP supports it), probe `server/discover` with 1000 ms timeout. Errors `[-32020, -32021, -32022]` identify modern servers and propagate; other failures fall back to `initialize` (`2025-11-25`) and initialized notification. Discovery versions must include the requested version. Legacy transport uses negotiated version/session headers, optional `DELETE` on close, inbound GET SSE (405 means unavailable), Last-Event-ID resumption, and session-expiry notification on session-bearing 404. Modern transport sends method/name/parameter headers. Reject redirects by default; choose transport semantics only after determining the generation.

[Decision] Implement both generations. `protocol_discovery` defaults on with `discovery_timeout` 1 s. Successful modern common-version discovery runs sessionless 2026-07-28 without `initialize`. Other failures, timeout, unsupported/disabled discovery, or a legacy common version use `initialize` requesting 2025-11-25 (accepting any supported returned version), then initialized, sessions, inbound GET, resumption, and `DELETE`. Both share `McpTransport`. Store generation per client, not in a global `origin` cache (revised 2026-09-14): each client owns a connection, and one bounded probe avoids a global registry. `ElicitationHandler` serves both legacy server requests and modern MRTR, retrying original requests with responses as below.

### 2.2.2 MRTR result structure

[Fact] (PV-030, closed 2026-09-14) The MCP schema at `https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/2026-07-28/schema.ts` defines:

- `Result { _meta?, resultType: ResultType }`, with `ResultType = "complete" | "input_required"`; `InputRequiredResult extends Result { inputRequests?: InputRequests; requestState?: string }`, requiring at least one of those fields.
- `InputRequests = { [key: string]: InputRequest }`; requests are `CreateMessageRequest | ListRootsRequest | ElicitRequest` with `method`/`params`. `InputResponses` maps the same keys to corresponding create-message, list-roots, or elicit results.
- `InputResponseRequestParams extends RequestParams { inputResponses?: InputResponses; requestState?: string }`; retry the original method/parameters with these fields and a new JSON-RPC ID.
- Request metadata contains optional progress token/client info, required protocol version/client capabilities under `io.modelcontextprotocol/*`; result metadata may contain server info. `DiscoverResult { supportedVersions: string[], capabilities: ServerCapabilities, instructions? }`. Errors: `-32020 HeaderMismatch`, `-32021 MissingRequiredClientCapability`, `-32022 UnsupportedProtocolVersion { data: { supported, requested } }`.

[Decision] Missing modern `resultType` is `McpError::Protocol`. For `input_required`, invoke the elicitation handler per keyed request, accepting only `elicitation/create`; sampling and roots return protocol errors because they are outside scope. Retry original parameters with keyed `inputResponses` and returned `requestState`, up to `max_input_rounds` (default 8). Missing handlers return `McpError::Elicitation`. Read server info from discovery `_meta["io.modelcontextprotocol/serverInfo"]`.

### 2.3 Client

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

[Decision] Each method takes explicit `RequestOptions` (timeout, total timeout, cancellation, per-request headers). Use the minimum of `timeout.or(default_request_timeout)` and `max_total_timeout`. On timeout/cancel, remove pending state and send cancellation notification. Accessors such as `server_capabilities()` clone optional state protected by a mutex. Closing any client clone fails all pending requests with `McpError::Closed`.

### 2.4 Tool bridging

[Decision] Convert MCP `text`/`image`/`audio`/`resource` `content` into tool output, honoring `isError`. `to_model_output` maps `content` into multimodal model output so capable providers receive images/`audio` directly.

```rust
#[non_exhaustive]
pub struct ToolsOptions {
    pub schemas: ToolSchemas,                  // Automatic (dynamic tools) | Explicit(HashMap<String, ToolSchemaPair { input, output: Option<Schema> }>)
    pub request_timeout: Option<Duration>,
    pub name_prefix: Option<String>,           // avoid collisions across servers
}
```

[Decision] Automatic tools are dynamic, completing `inputSchema` with empty properties and `additionalProperties: false`, validated by `Schema::from_json_schema`. Explicit schemas create function tools and include only named tools; optional `output` schemas validate structured `content` or JSON from the first `text` item, failing with `ToolError::Message`. Error results become `ToolError::Json(CallToolResult)`. `mcp_to_model_output` maps arrays to `ToolResultOutput::Content`: `text` stays `text`, `image`/`audio` base64 becomes files (default PNG/WAV), other items become JSON `text`. Non-arrays become JSON `output`. Metadata is `{clientName, toolName, title?, annotations?, app?, meta?}`. Collect header bindings only for modern transports supporting them; discard invalidly bound tools and report via `on_uncaught_error`.

### 2.5 OAuth

[Decision] OAuth implements authorization/protected-resource metadata discovery (RFC 8414/9728), dynamic registration (7591), PKCE authorization code flow (7636), and refresh. Applications implement storage and browser redirects through `OAuthClientProvider`; `Unauthorized` indicates user authorization is required.

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

[Decision] Storage methods return `Result`. Secret fields in `OAuthTokens`/`OAuthClientInformation` use `SecretString`, without `Serialize`; persistence requires `expose_to_json()`. Flow: path-aware protected-resource discovery then root fallback; RFC 8414 authorization discovery then OpenID (must advertise `S256`); registration if needed; code exchange or refresh; otherwise PKCE `S256` redirect. Protocol refresh errors propagate, while server/network errors fall back to authorization. Invalid/unauthorized client invalidates all credentials and retries once; invalid grant invalidates tokens and retries once. Validate every endpoint through `AuthOptions::url_policy` to reject internal metadata targets. HTTP runs `auth` once per `401`, coordinating concurrent failures; redirect returns `Unauthorized`.

## 3. Relationship to the core

- `McpClient::tools()` returns an ordinary `ToolSet`, mergeable with local tools for generation.
- Core tool approval, timeouts, and telemetry apply equally to MCP tools.
- `fingerprint_app_resource`/`detect_app_resource_drift` combine with tool fingerprints for cross-request consistency.

## 4. Example

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

## 5. Verification items

- [Fact] (PV-018, closed) Windows pipes/signals passed in `windows-2025`, first complete CI run 34797869442 on 2026-09-14: `missing_commands_fail_to_start`, `negotiates_lists_and_calls_tools_over_stdio`, and `server_requests_are_answered_over_stdio`, using a Python fixture server. [Decision] Use Tokio Command with `kill_on_drop(true)` and one task writing complete stdin frames (revised from locked writing in [ADR 0015](../04-decisions/2026-09-14-0015-mcp-stdio-frame-writer.md)). Windows creation flags suppress console windows; reject newlines in commands/arguments.
- [Fact] (PV-019) Elicitation params are `{ message: string, requestedSchema: object }`; results are `{ action: 'accept' | 'decline' | 'cancel', content?: object }`. [Decision] Preserve schema as JSON without structural validation: `ElicitationRequest { message: String, requested_schema: JsonValue }`, `ElicitResult { action: ElicitAction, content: Option<JsonObject> }`.
- [Fact] (PV-030, closed 2026-09-14) MRTR fields are fixed from the official schema (section 2.2.2), implemented and covered by mock-transport `input_required_*` tests in `tests/suite/client_modern.rs`.

## 6. Implementation record (2026-09-14, ferrin-mcp)

- [Fact] Layout follows section 2; largest file is `protocol/types.rs` at 630 lines, others below 600. Dependencies: specification, schema, tool, provider utilities (`reqwest`), Tokio `rt`/`sync`/`time`/`macros` (`stdio` adds `process`/`io-util`), `tokio-util`, `futures-util`, `serde`/`serde_json`, `thiserror`, `bytes`, `http`, `url`, `sha2`, `base64`, `rand`, `secrecy`, `tracing`; no `ferrin-message`. Default `stdio`/`oauth` features pass `cargo hack --each-feature`.
- [Fact] Seventy-two all-feature tests, 59 without features, cover JSON-RPC, `headers`, modern discovery/metadata/results/MRTR/capabilities/retries/timeouts/server requests/errors/close, legacy fallback/version errors, resources/prompts, `tools`, Apps, HTTP JSON/SSE/session/202/405/404/`401`/500/content types/redirects/modern `headers`/`DELETE`/URL policy, legacy SSE, `stdio` negotiation/`tools`/environment/server requests, and OAuth challenge parsing/discovery/PKCE RFC vectors/URLs/resource selection/registration/exchange/refresh/invalid-grant retries/basic auth/missing `S256`/`401` flows. Two request snapshots live in `tests/suite/snapshots/`. Client tests use `MockTransport` in `tests/suite/common.rs` because fixed fixture responses cannot echo dynamic JSON-RPC IDs; `stdio` uses `tests/fixtures/stdio/echo_server.py`.
- [Fact] Added `Fixture::hold_open()` in `ferrin-testing` so event streams remain open after the last event until connection/server closure.
- [Decision] HTTP/SSE policies default to HTTPS-only/private-network rejection; validate and pin endpoint addresses at start, then attach pinned addresses to every request. Local development explicitly enables HTTP/private networks. Default body/event limits are 16 MiB; OAuth bodies 1 MiB. `Debug` redacts headers and omits session IDs/tokens.
- [Decision] Box `McpError::Transport(Box<TransportFailure>)` to meet the 128-byte limit. Retry tool calls for HTTP 408/409/429/5xx, retryable connection failures, and I/O; never JSON-RPC errors.
- [Decision] Streamable HTTP rejects redirects by default; `Follow` accepts only same-origin `307`/`308` within hop limits. Reconnect legacy inbound GET with 1000 ms initial delay, factor 1.5, 30 s cap, at most 2 retries, and Last-Event-ID; `405` means unavailable. Legacy sessions `DELETE` on close by default; modern clients do not.
- [Decision] Legacy SSE startup waits for a same-origin `endpoint` event. Stream termination emits `Error` then `Closed`; later sends return `McpError::Closed`.
- [Decision] stdio clears environment, inherits only allowlisted values not beginning with `()` (Unix `HOME`/`LOGNAME`/`PATH`/`SHELL`/`TERM`/`USER`; Windows in the constant), then overlays configured env. Stderr is inherited unless Null; close starts kill then waits. See [ADR 0015](../04-decisions/2026-09-14-0015-mcp-stdio-frame-writer.md) for writing.
- [Decision] Server `ping` gets `{}`; elicitation invokes its handler (missing: `-32601`, invalid params: `-32602`, handler failure: `-32603`). Other methods, including sampling/roots, return `-32601`. Notifications go to `on_notification`, otherwise debug logging.
- [Decision] Added after live validation on 2026-09-14: automatic server-schema tools use `ToolBuilder::strict(false)`; explicit typed schemas are unchanged. OpenAI Responses defaults to strict function schemas requiring all properties in `required` and no extra properties; common MCP schemas, including server-everything, fail these requirements and otherwise reject the whole call.
- [Decision] Outside scope: modern `subscriptions/listen`, resumption tokens beyond Last-Event-ID (`SendOptions` has no resumption token), sampling/roots, and OAuth `client_secret_jwt`/`private_key_jwt`.


[Decision] 2026-09-15 client task ownership belongs to public client handles, separately from state shared by dispatch and server-request futures, so dropping the final handle aborts the dispatcher and its handlers. Built-in transport destructors cancel their streams and abort owned tasks, breaking internal task/state cycles; explicit close retains protocol session termination. Failed startup closes its transport (regression coverage: `crates/ferrin-mcp/tests/suite/client_lifecycle.rs`).

[Decision] 2026-09-15 one effective deadline covers transport send, response receipt and every `input_required` round/handler. Request cancellation also interrupts send; dropping a request removes its pending registration and cancels its private transport token. Timeout/cancellation notifications run as best-effort tasks owned by public client handles, with a separate one-second bound; weak ownership prevents a cycle and final client drop aborts cleanup, so stalled notification delivery cannot delay the original error (regression coverage: `crates/ferrin-mcp/tests/suite/client_deadlines.rs`, paused Tokio time). Failed startup/initialization also attempts transport cleanup under a separate one-second bound, preserving the original failure even if custom close or HTTP session DELETE stalls. Local pending work is stopped before awaiting protocol cleanup.

[Decision] 2026-09-15 request-scoped HTTP SSE pumps observe both `SendOptions.cancellation` and transport cancellation. Ending the client request cancels its private token, releasing the body even while the transport stays open (regression coverage: `crates/ferrin-mcp/tests/suite/http_stream_lifecycle.rs`, body-drop notifications).

[Decision] 2026-09-15 `TransportEvent::RequestError { id, error }` attributes response-stream failures to one pending request. HTTP request SSE parsing stops after a matching JSON-RPC response/error; EOF without one, invalid messages and body/decode errors fail that request even when no timeout is configured. Notifications and other request IDs do not count as completion (source: `crates/ferrin-mcp/src/transport/http_stream.rs`).
