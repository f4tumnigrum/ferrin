# HTTP 传输与安全

[English](../../01-architecture/14-http-and-security.md) | **简体中文**

位于 `ferrin-provider-util`。本层供供应商适配器与核心层的下载功能使用，应用代码通常不直接调用。

## 1. 传输抽象

【事实】应用需要替换 SDK 的 HTTP 实现：企业代理、录制回放测试、特殊运行时（自定义 TLS、受限网络）都要求传输层可注入。

【决策】定义 `HttpTransport` trait，默认实现基于 reqwest 0.13.5：

```rust
pub trait HttpTransport: Send + Sync + 'static {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;
}

pub struct HttpRequest {
    pub method: http::Method,
    pub url: Url,
    pub headers: Headers,
    pub body: RequestBody,           // Empty | Json(Bytes) | Form(MultipartForm) | Bytes(Bytes)
    pub cancellation: CancellationToken,
    pub timeout: Option<Duration>,
}

pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: Headers,
    pub body: BoxStream<'static, Result<Bytes, TransportError>>,
}

pub struct ReqwestTransport { client: reqwest::Client }
```

依据：trait 形态使测试可以注入录制回放传输而不依赖网络，也让请求构造逻辑独立于具体 HTTP 客户端。

`ReqwestTransport::default()` 配置：rustls TLS、HTTP/2 优先、连接池、禁用自动重定向（重定向由安全下载逻辑显式处理）、无默认超时（超时由核心层令牌控制）、`User-Agent` 由请求头提供。

【事实】2026-09-13 实现（`crates/ferrin-provider-util/src/http/`）与上述草案的差异：

- `RequestBody` 为 `Empty | Bytes { content_type, data } | Multipart(MultipartForm)`；JSON 体通过 `RequestBody::json(bytes)` 构造为带 `application/json` 的 `Bytes`，`MultipartForm` 由本 crate 自行编码（`--boundary`、`Content-Disposition`、`Content-Type` 逐部件写出，随机或固定边界），不使用 reqwest 的 `multipart` feature。
- `HttpRequest` 增加 `pinned_addresses: Vec<SocketAddr>`：非空时传输层必须只连接这些地址（`ReqwestTransport` 为此构建带 `resolve_to_addrs` 的专用客户端）。
- `HttpResponse` 提供 `from_bytes`、`from_stream`、`head()`；`TransportError { kind: TransportErrorKind, message, cause }`，`TransportErrorKind` 为 `Connect | Timeout | Reset | Io | Tls | InvalidUrl | InvalidRequest | Body | BodyTooLarge | Cancelled | Other`，`is_retryable()` 只对前四种为真。
- `ReqwestTransport::new()` 可失败（TLS 后端初始化），`builder()` 暴露带默认配置的 `ClientBuilder`，`from_client()` 接受外部客户端；`default_transport()` 返回进程级共享的 `Arc<dyn HttpTransport>`。取消令牌同时作用于请求发送与响应体流（体流被取消时产出 `TransportErrorKind::Cancelled` 项后结束）。
- reqwest 的 `Error` 按 `is_timeout/is_connect/is_body/is_decode/is_builder/is_request` 映射到上述分类；`is_request` 且错误链文本含 `reset`/`broken pipe`/`connection closed` 时归为 `Reset`。

## 2. 请求辅助

【决策】请求辅助函数（JSON、表单与原始体的 POST，以及 GET）：发送请求、去除值为 `None` 的头、按状态码选择失败处理器或成功处理器、把网络错误包装为 `ApiCallError`（可重试性由错误类型推断）、把空响应体包装为 `EmptyResponseBody` 错误。依据：所有适配器共享同一条请求路径，错误分类与重试判定只需实现一次。

```rust
pub async fn post_json<T>(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    body: &impl Serialize,
    handlers: ResponseHandlers<T>,
    cancellation: CancellationToken,
) -> Result<ApiResponse<T>, ProviderError>;

pub struct ApiResponse<T> { pub value: T, pub response_headers: Headers, pub raw_body: Option<String> }

pub struct ResponseHandlers<T> {
    pub success: Box<dyn ResponseHandler<T>>,
    pub failure: Box<dyn ResponseHandler<ProviderError>>,
}
```

【事实】2026-09-13 实现：`post_json`/`post_form`/`post_bytes`/`get`/`delete` 均以 `&ResponseHandlers<T>` 借用处理器（同一组处理器可跨请求复用），底层为 `send(transport, HttpRequest, request_body: Option<JsonValue>, handlers)`；`ApiResponse<T>` 的第三个字段为 `raw: Option<JsonValue>`（成功处理器解析出的原始 JSON 值），不保留原始文本。请求缺少 `Content-Type` 时按体类型补齐。传输失败映射为 `ApiCallError { message: "cannot connect to API: ...", is_retryable: TransportError::is_retryable() }`；处理器返回的非 `ApiCall` 错误（如 `TypeValidationError`）被包装为带状态码与响应头的 `ApiCallError`，`ApiCall` 与 `Cancelled` 原样透出。

【决策】2026-09-13 在 `ferrin_spec::error::ProviderError` 增加 `Cancelled` 变体（`is_retryable() == false`，`kind_name() == "cancelled"`）；传输层的 `TransportErrorKind::Cancelled` 在请求辅助与响应处理器中一律转换为它。依据：适配器只能返回 `ProviderError`，若把取消包装为 `ApiCallError`，核心层无法区分“调用方取消”与“网络失败”，会触发重试或把取消记为供应商错误；Rust 中以专用变体表达“已取消”，不依赖错误消息或类型名判定。核心层把 `Provider(ProviderError::Cancelled)` 归并为 `Error::Cancelled`（见[错误模型](12-error-model.md)第 2.1 节）。

## 3. 响应处理器

【决策】响应处理器族：JSON 响应处理器（解析并校验 JSON）、JSON 错误响应处理器（错误 JSON → `ApiCallError`，可自定义消息提取与可重试判定）、SSE 响应处理器（逐事件解析，`[DONE]` 跳过）、JSON Lines 响应处理器、二进制响应处理器、二进制流响应处理器、状态码错误处理器；解析失败的 SSE 事件产生错误项而不中断流。依据：供应商响应形态只有这几种，处理器族让适配器以声明方式组合而不重复解析代码。

```rust
pub fn json_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<T>;
pub fn json_error_response_handler<E: DeserializeOwned>(to_message: fn(&E) -> String, is_retryable: Option<fn(&HttpResponseHead, &E) -> bool>) -> impl ResponseHandler<ProviderError>;
pub fn event_source_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<BoxStream<'static, ParseResult<T>>>;
pub fn json_lines_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<BoxStream<'static, ParseResult<T>>>;
pub fn binary_response_handler() -> impl ResponseHandler<Bytes>;
pub fn binary_stream_response_handler() -> impl ResponseHandler<BoxStream<'static, Result<Bytes, TransportError>>>;

pub enum ParseResult<T> { Ok { value: T, raw: JsonValue }, Err { error: JsonParseError | TypeValidationError, raw: String } }
```

`ParseResult::Ok` 携带原始 JSON 值，供 `include_raw_chunks` 输出 `StreamPart::Raw`。

【事实】2026-09-13 实现：

- `ResponseHandler<T>::handle(&self, ResponseContext { url, request_body }, HttpResponse) -> BoxFuture<'static, Result<Handled<T> { value, raw: Option<JsonValue>, headers }, ProviderError>>`。处理器以构建器形态提供：`json_response_handler::<T>().with_max_bytes(n)`、`json_error_response_handler::<E>(to_message).with_is_retryable(|head, parsed: Option<&E>| ...)`、`text_response_handler()`、`status_code_error_response_handler()`、`binary_response_handler().with_max_bytes(n)`、`binary_stream_response_handler()`、`event_source_response_handler::<T>().with_max_event_bytes(n)`、`json_lines_response_handler::<T>()`。
- `ParseResult<T>` 为 `Ok { value, raw: JsonValue } | Err { error: ProviderError, raw: Option<String> }`（`raw` 为收到的分片文本，传输错误时为 `None`），`into_result()` 丢弃原始载荷。
- 响应体读取 `read_body(headers, stream, max_bytes)` 默认上限 2 GiB（`DEFAULT_MAX_RESPONSE_BYTES`），`Content-Length` 超限时在读取前失败，超限映射为 `TransportErrorKind::BodyTooLarge`（不可重试）。
- 错误处理器：响应体为空或不能解析为 `E` 时，消息回落为状态码的标准原因短语（如 `Service Unavailable`），`data` 为空；解析成功时 `data` 为原始 JSON；`is_retryable` 默认由状态码决定（`ApiCallError::with_status`），`with_is_retryable` 可覆盖。
- 流式处理器（SSE、JSON Lines）在 `content-length: 0` 时返回 `EmptyResponseBodyError`；SSE 跳过 `data: [DONE]`；JSON Lines 按 `\n` 切分并去除尾部 `\r`，忽略空行；两者的传输错误与解析错误都作为 `ParseResult::Err` 项产出，流不中断。

## 4. SSE 解码

【事实】OpenAI 风格的 SSE 流以 `data: [DONE]` 事件标记结束，该事件不是 JSON，解析器必须跳过它。

【决策】`ferrin_provider_util::sse::SseDecoder` 自行实现 WHATWG EventSource 解析（字段 `event`、`data`、`id`、`retry`，多行 `data` 以 `\n` 连接，注释行忽略，`\r\n`/`\r`/`\n` 三种行尾，UTF-8 BOM 剥离）。依据：解析规则固定且短小，自实现便于加入每事件到达时间戳（性能指标需要）与最大事件体积限制；`eventsource-stream` 0.2.3 自 2022 年未更新。

【事实】2026-09-13 实现：`SseDecoder::feed(&[u8]) -> Result<Vec<SseEvent>, SseError>` 增量解码（跨分片的行与 `\r\n` 均正确处理），`finish()` 丢弃未以空行结束的事件（规范要求）；`SseEvent { event: Option<String>, data, id: Option<String>, retry: Option<Duration>, received_at: Option<Instant> }`，`id` 含 NUL 时忽略，`retry` 仅接受纯数字；默认单事件上限 `DEFAULT_MAX_EVENT_BYTES` = 16 MiB，超限返回 `SseError::EventTooLarge`。`sse::decode_stream(body, max_event_bytes)` 把响应体流转换为事件流并填充 `received_at`，首个错误（传输或解码）后结束。

## 5. 可重试性分类

【决策】`ApiCallError::is_retryable` 默认：状态码 408、409、429 或 ≥ 500；网络层错误视为可重试；适配器可在错误处理器中按响应体覆盖（如 Anthropic 流内错误 `overloaded_error` 对应 529、可重试，`request_too_large` 对应 413、不可重试）。依据：这些状态码在各供应商文档中标注为瞬时；流内错误帧没有 HTTP 状态码，只能按错误类型推断。

```rust
pub fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}
```

`TransportError::{Connect, Timeout, Reset, Io}` 转换为 `ApiCallError { is_retryable: true }`；`TransportError::Tls`、`InvalidUrl` 为不可重试。

【事实】2026-09-13 实现的 `retry` 模块另提供 `retry_after(&Headers) -> Option<Duration>`（优先 `retry-after-ms` 毫秒，其次 `retry-after` 的秒数或 HTTP 日期）与 `retry_after_within(&Headers, max)`（超过上限时忽略，供核心层重试策略使用，窗口为 60 s）。

## 6. 设置与凭据加载

【决策】API 密钥与设置的加载：显式参数优先，其次环境变量（经 `ferrin_provider_util::settings` 读取），缺失时返回 `LoadApiKey`/`LoadSetting` 错误并说明应设置哪个参数或环境变量。依据：缺少密钥是最常见的首次接入失败，错误消息应直接给出修复方法。

```rust
pub fn load_api_key(config: ApiKeyConfig<'_>) -> Result<SecretString, LoadApiKeyError>;
pub fn load_setting(config: SettingConfig<'_>) -> Result<String, LoadSettingError>;
pub fn load_optional_setting(config: SettingConfig<'_>) -> Option<String>;
```

【事实】2026-09-13 实现：`load_optional_setting(value: Option<String>, environment_variable: &str) -> Option<String>` 不使用配置结构（无错误消息可生成）；`ApiKeyConfig { api_key: Option<SecretString>, environment_variable, parameter_name, description }`、`SettingConfig { value: Option<String>, environment_variable, setting_name, description }`；错误消息格式为 `"{description} API key is missing. Pass it using the '{parameter}' parameter or the {ENV} environment variable."`。`settings::env_var(name)` 是工作区内唯一读取进程环境的位置（局部放行 `clippy::disallowed_methods`），非 UTF-8 值视为缺失。

【决策】密钥类型为 `secrecy::SecretString`（0.10.3），`Debug` 输出遮蔽；只在构造 `Authorization` 头时 `expose_secret()`。适配器把凭据加载放在请求构造阶段的 `headers()` 闭包中执行（【事实】`createOpenAI` 的 `getHeaders` 惰性闭包），使 `create_openai()` 不因缺少环境变量而失败。

## 7. 供应商选项解析

【决策】供应商选项解析：取 `provider_options[provider]` 并按类型反序列化，失败返回 `InvalidArgument` 错误；键不存在时返回 `None`。依据：其他供应商的键在本适配器中无意义，忽略而非报错使同一份选项可以跨供应商复用。

```rust
pub fn parse_provider_options<T: DeserializeOwned + JsonSchema>(
    provider_key: &str,
    options: &ProviderOptions,
) -> Result<Option<T>, InvalidArgumentError>;
```

【决策】2026-09-13 实现的约束为 `T: DeserializeOwned`，不要求 `JsonSchema`：选项类型是适配器内部结构，serde 反序列化即完成校验，错误消息 `invalid {provider_key} provider options: {serde error}` 以 `InvalidArgumentError { argument: "provider_options", cause }` 返回。由此 `ferrin-provider-util` 不依赖 `ferrin-schema`（[Crate 划分](02-crates.md)第 2 节已同步）。

## 8. 安全 URL 处理

【决策】安全 URL 规则（依据：这些规则共同封堵 SSRF 与 DNS 重绑定）：

- 应用或模型提供的 URL 在下载前必须通过校验：仅允许 `http`/`https`（默认仅 `https`，可配置），拒绝解析到回环、链路本地、私有网段、多播与保留地址的主机名，拒绝含凭据的 URL。
- 重定向不自动跟随；每一跳重新校验目标 URL；限制最大跳数。
- 通过 DNS 固定（解析一次后连接固定地址）防止解析结果在校验后变化（DNS rebinding）。
- `trusted_origins`/`credentialed_origins` 白名单允许对已知来源放宽限制或携带凭据。
- 下载体积上限 100 MiB，超出中止；lint 规则要求所有出站请求位于经审计的入口。

【决策】`ferrin_provider_util::secure_url` 实现：

```rust
pub struct UrlPolicy {
    pub allowed_schemes: Vec<Scheme>,              // default [Https]
    pub allow_private_networks: bool,              // default false
    pub trusted_origins: Vec<Origin>,              // skip private-network check
    pub credentialed_origins: Vec<Origin>,         // may receive Authorization header
    pub max_redirects: u8,                         // default 5
    pub max_body_bytes: u64,                       // default 100 MiB
}

pub async fn validate_url(url: &Url, policy: &UrlPolicy) -> Result<ValidatedUrl, UrlValidationError>;
pub async fn fetch(transport: &dyn HttpTransport, url: Url, policy: &UrlPolicy, cancellation: CancellationToken) -> Result<Downloaded, DownloadError>;
```

- 私网判定使用 `ipnet` 2.12.2 的网段表（IPv4：`10/8`、`172.16/12`、`192.168/16`、`127/8`、`169.254/16`、`0/8`、`100.64/10`、`224/4`、`240/4`；IPv6：`::1`、`fc00::/7`、`fe80::/10`、`::ffff:0:0/96` 映射地址按 IPv4 规则）。
- DNS 解析使用 `tokio::net::lookup_host`，全部解析结果都必须通过判定；随后通过 reqwest `ClientBuilder::resolve_to_addrs` 固定地址发起连接。
- 重定向由 `fetch` 手动处理：读取 `Location`，合并为绝对 URL，重新 `validate_url`，跨源重定向剥离 `Authorization`。
- 体积限制在流式读取时累计检查，超限立即中止连接。

【事实】2026-09-13 实现（`crates/ferrin-provider-util/src/secure_url/`）：

- `UrlPolicy` 字段如上，另有构建器方法 `allow_http()`、`allow_private_networks()`、`trust_origin(&Url)`、`credential_origin(&Url)`、`max_redirects(n)`、`max_body_bytes(n)`；`Scheme` 为 `Https | Http`；`ValidatedUrl { url, addresses: Vec<SocketAddr> }`。
- 校验顺序：scheme → 嵌入凭据 → 主机存在 → 受信来源直接放行（不解析 DNS）→ 主机名规则（`localhost`、`*.localhost`、`*.local` 与 IP 字面量）→ `tokio::net::lookup_host` 解析 → 全部地址通过网段判定。`allow_private_networks` 只跳过网段判定，仍解析并固定地址。
- 网段表按 IANA 特殊用途地址登记扩展：IPv4 增加 `192.0.0/24`、`192.0.2/24`、`198.18/15`、`198.51.100/24`、`203.0.113/24`；IPv6 为 `::`、`::1`、`fc00::/7`、`fe80::/10`、`fec0::/10`、`ff00::/8`、`2001:db8::/32`、`3fff::/20`，且 IPv4 映射（`::ffff:a.b.c.d`）、IPv4 兼容（`::a.b.c.d`）、NAT64（`64:ff9b::/96`、`64:ff9b:1::/48`）与 6to4（`2002::/16`）地址按内嵌 IPv4 再判定一次。判定函数 `is_private_ip`、`is_private_hostname` 公开。
- `fetch_with_headers(transport, url, headers, policy, cancellation)`：先删除 `BLOCKED_DOWNLOAD_HEADERS`（逐跳、代理、云元数据与 Cookie 头），`Authorization` 只发送给 `credentialed_origins` 中的来源；缺省补 `User-Agent: ferrin/<version>`；3xx 响应按 `Location` 合并绝对 URL，超过 `max_redirects` 报错，跨源跳转只保留 `User-Agent` 与 `Accept`（保留 `Accept` 以便内容协商在跨 CDN 跳转后仍有效）；非 2xx 为 `Status` 错误；体流经 `read_body` 按 `max_body_bytes` 限制。`fetch` 为无额外头的便捷形式。
- `Downloaded { url, data, media_type: Option<MediaType>（Content-Type 去参数）, headers }`；`DownloadError { url: Box<Url>, kind: DownloadErrorKind }`，`DownloadErrorKind` 为 `Validation(UrlValidationError) | Transport(TransportError) | Status { status, headers } | InvalidRedirect | TooManyRedirects { limit } | Cancelled`；URL 装箱是为满足 `large-error-threshold = 128`。

【决策】通过 Clippy `disallowed-methods` 禁止在 `secure_url` 模块之外直接调用 `reqwest::Client::get/post/execute`（配置见[编码规范](../03-engineering/03-coding-standards.md)）。

## 9. ID 生成

【决策】ID 生成器生成 `<prefix>-<随机字母数字>`（默认 16 位，分隔符 `-`）；核心层以 24 位随机部分生成文本 ID 与调用 ID。依据：带前缀的 ID 在日志中可辨识来源；24 位字母数字随机部分的碰撞概率可忽略。

```rust
pub trait IdGenerator: Send + Sync { fn generate(&self) -> String; }
pub struct PrefixedIdGenerator { prefix: &'static str, size: usize }   // alphanumeric via rand 0.10 ThreadRng
```

核心层默认前缀：`ftxt`（文本部件）、`call`（调用）、`appr`（审批）、`tool`（工具调用 ID 兜底）。测试通过 `ferrin_testing::SequentialIdGenerator` 获得确定性 ID。

【事实】2026-09-13 实现：`PrefixedIdGenerator::new(prefix: impl Into<String>, size)`（前缀为 `Option<String>`，`unprefixed(size)` 无前缀，`with_separator(char)` 更换分隔符，默认 `-`），字母表 `0-9A-Za-z`，随机源 `rand::rng()`；`generate_id()` 生成 16 位无前缀 ID；任何 `Fn() -> String + Send + Sync` 闭包自动实现 `IdGenerator`。

## 10. User-Agent

【决策】`with_user_agent_suffix` 在现有 `user-agent` 值后追加空格分隔的标识；链路为应用头 → `ferrin/<version>` → `ferrin-<provider>/<version>`。依据：HTTP User-Agent 语法允许多个产品标识，追加而非覆盖保留应用自己的标识。

Ferrin 链路：应用头 → `ferrin/<core-version>` → `ferrin-<provider>/<provider-crate-version>`（Agent 额外前置 `ferrin-agent/tool-loop`）。

## 11. 待验证

- 【事实】（PV-015，`verification/pv015-reqwest`）reqwest 0.13.5 保留 `ClientBuilder::resolve_to_addrs`、`redirect(redirect::Policy::none())`、`https_only`，并新增 `http1_max_headers`（默认 100）与 `Error::is_dns()`（0.13.5 变更日志）。构建 100 个固定地址客户端耗时 5.8 ms（每个约 58 µs）；对不可达固定地址的请求在超时后失败且 `is_dns() == false`，证明未再进行 DNS 解析。0.13 的破坏性变更：默认 TLS 后端改为 rustls（feature 名由 `rustls-tls` 改为 `rustls`），默认加密提供者为 aws-lc，默认证书校验器为 `rustls-platform-verifier`，`query`/`form` 变为可选 feature，TLS 方法改名（旧名软弃用）。
- 【决策】每个下载目标构建专用客户端的方案成立（成本在微秒级、内存随请求生命周期释放），不引入 hyper-util 直连方案。`ReqwestTransport` 的 features 原定为 `rustls`、`http2`、`stream`、`json`、`multipart`、`charset`（2026-09-13 修订，见下一条）。
- 【决策】2026-09-13 实现后 reqwest 只启用 `rustls`（经工作区 feature）、`http2`、`stream`：JSON 序列化由 `serde_json` 直接完成，multipart 由本 crate 编码（见第 1 节），响应体一律按字节读取后有损转换为 UTF-8，`charset` 解码无用武之地。依据：少启用三个 feature 可去掉 `mime_guess`、`encoding_rs` 等传递依赖，并让请求体编码在传输实现之间保持一致（测试传输与录制传输看到与 reqwest 完全相同的字节）。
- 【事实】（PV-016，`verification/pv016-header-values`）`http` 1.5.0 的 `HeaderValue::from_str` 与 `from_bytes` 都接受 0x80–0xFF 字节（UTF-8 文本可直接构造），拒绝换行与 DEL，接受制表符；`HeaderValue::to_str()` 对含非 ASCII 字节的值返回错误，需用 `as_bytes()` 读取。
- 【决策】`Headers::insert(&str, &str)` 使用 `HeaderValue::from_str`；响应头读取接口提供 `get_str()`（仅 ASCII）与 `get_bytes()`，供应商元数据中的头值以 UTF-8 有损转换后保存。第一方供应商的请求头均为 ASCII，不受影响。

【事实】2026-09-15 重试头解析采用可失败的时长转换，忽略非有限、负数及超出范围的数值；无效的毫秒头仍允许回退到秒数或日期头（来源：`crates/ferrin-provider-util/tests/suite/misc.rs`）。

【事实】2026-09-15 SSE BOM 检测在解析首行前完成，支持 BOM 跨块；回归测试枚举 BOM、CRLF 和事件分隔符之间的全部两处分块边界（来源：`crates/ferrin-provider-util/tests/suite/sse.rs`）。

【决策】2026-09-15 `json_lines_response_handler::<T>().with_max_line_bytes(n)` 限制每个物理行在 LF 前的字节数，包括结尾 CR（默认 16 MiB）。解析按块增量消费，不复制尚未处理的其他行；超限仅产生一次不可重试的 `BodyTooLarge` 错误并立即释放响应体。无终止换行和纯空白行同样受限，批结果总量仍采用流式处理（来源：`crates/ferrin-provider-util/src/http/json_lines.rs` 及其回归测试）。

【决策】`WebSocketConfig` 的调试输出对完整 URL 和协议列表脱敏，因为 Realtime 凭据可能出现在任一位置。连接值保持不变；调试格式不能泄露传给供应商 `websocket_config` 的令牌。
