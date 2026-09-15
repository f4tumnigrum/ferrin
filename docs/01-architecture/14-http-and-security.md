# HTTP transport and security

**English** | [Chinese](../zh-CN/01-architecture/14-http-and-security.md)

Implemented in `ferrin-provider-util`, for provider adapters and core downloads; applications rarely call this layer directly.

## 1. Transport abstraction

[Fact] Enterprise proxies, replay tests, and custom TLS or restricted networking require injectable HTTP implementations.

[Decision] Define `HttpTransport`, with a default based on reqwest 0.13.5:

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

A trait allows replay transports without network access and separates request construction from the HTTP client.

Default `ReqwestTransport` configuration: rustls, HTTP/2 preference, pooling, no automatic redirects (secure downloads handle them), no default timeout (core tokens control it), and request-supplied `User-Agent`.

[Fact] The 2026-09-13 implementation (`crates/ferrin-provider-util/src/http/`) differs from the draft:

- `RequestBody` is `Empty | Bytes { content_type, data } | Multipart(MultipartForm)`. `RequestBody::json(bytes)` constructs JSON-typed bytes. The crate encodes `multipart` boundaries and per-part `Content-Disposition`/`Content-Type` itself, with random or fixed boundaries, without reqwest's `multipart` feature.
- `HttpRequest` adds `pinned_addresses: Vec<SocketAddr>`. When nonempty, transports must connect only to these addresses; reqwest builds a dedicated `resolve_to_addrs` client.
- `HttpResponse` offers `from_bytes`, `from_stream`, and `head()`. `TransportError { kind, message, cause }` uses kinds `Connect | Timeout | Reset | Io | Tls | InvalidUrl | InvalidRequest | Body | BodyTooLarge | Cancelled | Other`; only the first four are retryable.
- `ReqwestTransport::new()` can fail during TLS initialization; `builder()` exposes a preconfigured `ClientBuilder`, and `from_client()` accepts an external client. `default_transport()` shares a process-wide `Arc<dyn HttpTransport>`. Cancellation covers sending and response streaming; a cancelled body emits one cancelled error and ends.
- Map reqwest errors by `is_timeout/is_connect/is_body/is_decode/is_builder/is_request`; request errors whose chain contains `reset`, `broken pipe`, or `connection closed` become `Reset`.

## 2. Request helpers

[Decision] Helpers for JSON/form/raw POST and GET send requests, omit `None` headers, select success/failure handlers by status, wrap network errors as `ApiCallError` with classified retryability, and report empty bodies as `EmptyResponseBody`. One shared path centralizes classification.

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

[Fact] Implementation on 2026-09-13: `post_json`/`post_form`/`post_bytes`/`get`/`delete` borrow reusable `&ResponseHandlers<T>` and delegate to `send(transport, HttpRequest, request_body: Option<JsonValue>, handlers)`. `ApiResponse<T>` stores parsed `raw: Option<JsonValue>`, not raw text. Supply missing `Content-Type` from body type. Transport failures become `ApiCallError { message: "cannot connect to API: ...", is_retryable: TransportError::is_retryable() }`. Wrap non-API handler errors such as type validation with status/headers; pass `ApiCall` and `Cancelled` through.

[Decision] Add specification `Cancelled` on 2026-09-13, non-retryable with kind `cancelled`; request helpers and response handlers map transport cancellation to it. Wrapping cancellation as an API failure would confuse caller cancellation with network failure and trigger retries or wrong telemetry. The core maps it to `Error::Cancelled` ([Error model](12-error-model.md), section 2.1).

## 3. Response handlers

[Decision] Handler families cover validated JSON, JSON errors with configurable message/retry extraction, SSE (skip `[DONE]`), JSON Lines, binary, binary streams, and status errors. Malformed SSE events produce error items rather than aborting parsing. Adapters compose handlers instead of duplicating response logic.

```rust
pub fn json_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<T>;
pub fn json_error_response_handler<E: DeserializeOwned>(to_message: fn(&E) -> String, is_retryable: Option<fn(&HttpResponseHead, &E) -> bool>) -> impl ResponseHandler<ProviderError>;
pub fn event_source_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<BoxStream<'static, ParseResult<T>>>;
pub fn json_lines_response_handler<T: DeserializeOwned>() -> impl ResponseHandler<BoxStream<'static, ParseResult<T>>>;
pub fn binary_response_handler() -> impl ResponseHandler<Bytes>;
pub fn binary_stream_response_handler() -> impl ResponseHandler<BoxStream<'static, Result<Bytes, TransportError>>>;

pub enum ParseResult<T> { Ok { value: T, raw: JsonValue }, Err { error: JsonParseError | TypeValidationError, raw: String } }
```

`ParseResult::Ok` retains raw JSON for `include_raw_chunks` to emit `StreamPart::Raw`.

[Fact] Implementation on 2026-09-13:

- `ResponseHandler<T>::handle(&self, ResponseContext { url, request_body }, HttpResponse) -> BoxFuture<'static, Result<Handled<T> { value, raw: Option<JsonValue>, headers }, ProviderError>>`. Builders: `json_response_handler::<T>().with_max_bytes(n)`, `json_error_response_handler::<E>(to_message).with_is_retryable(|head, parsed: Option<&E>| ...)`, `text_response_handler()`, `status_code_error_response_handler()`, `binary_response_handler().with_max_bytes(n)`, `binary_stream_response_handler()`, `event_source_response_handler::<T>().with_max_event_bytes(n)`, and `json_lines_response_handler::<T>()`.
- `ParseResult<T>` is `Ok { value, raw: JsonValue } | Err { error: ProviderError, raw: Option<String> }`; error `raw` data is received chunk text, absent for transport failures. `into_result()` discards `raw` payloads.
- `read_body(headers, stream, max_bytes)` defaults to 2 GiB (`DEFAULT_MAX_RESPONSE_BYTES`), rejecting excessive `Content-Length` before reading. Excess size maps to non-retryable `TransportErrorKind::BodyTooLarge`.
- Error handlers fall back to the status reason phrase, such as `Service Unavailable`, with no `data` for empty/unparseable bodies; successful parsing retains raw JSON. Status-based retryability via `ApiCallError::with_status` can be overridden.
- SSE/JSON Lines reject `content-length: 0` with `EmptyResponseBodyError`. SSE skips `[DONE]`; JSON Lines splits on newline, trims trailing carriage returns, and ignores blank lines. Transport/parse failures yield `ParseResult::Err` items rather than immediately terminating the handler stream.

## 4. SSE decoding

[Fact] OpenAI-style SSE ends with non-JSON `data: [DONE]`, which parsers must skip.

[Decision] Implement WHATWG EventSource in `SseDecoder`: `event`, `data`, `id`, `retry`; newline-joined `data` lines; ignored comments; CRLF/CR/LF endings; UTF-8 BOM stripping. Small fixed rules permit timestamps and `event` limits; `eventsource-stream` 0.2.3 has not been updated since 2022.

[Fact] `SseDecoder::feed(&[u8]) -> Result<Vec<SseEvent>, SseError>` handles split lines/CRLF incrementally; `finish()` discards events lacking a terminating blank line per spec. `SseEvent { event: Option<String>, data, id: Option<String>, retry: Option<Duration>, received_at: Option<Instant> }` ignores NUL-containing IDs and accepts numeric-only retries. Default event limit is 16 MiB (`DEFAULT_MAX_EVENT_BYTES`), returning `SseError::EventTooLarge`. `sse::decode_stream(body, max_event_bytes)` timestamps events and ends after its first transport/decoding failure.

## 5. Retry classification

[Decision] API statuses 408, 409, 429, and ≥500 default to retryable; network failures are retryable. Adapters may override using body data, for example Anthropic `overloaded_error` → retryable 529 and `request_too_large` → non-retryable 413. Provider guidance identifies transient statuses; in-stream errors need type-based inference because they have no HTTP status.

```rust
pub fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}
```

`TransportError::{Connect, Timeout, Reset, Io}` map to retryable API errors; TLS and invalid URLs do not.

[Fact] The `retry` module also provides `retry_after(&Headers) -> Option<Duration>`, preferring milliseconds then seconds/HTTP dates, and `retry_after_within(&Headers, max)`, ignoring delays over the core's 60 s window.

## 6. Settings and credentials

[Decision] Explicit settings/keys take precedence over environment variables read through `settings`. Missing values return `LoadApiKey`/`LoadSetting`, naming the parameter or variable to set so initial integration errors are actionable.

```rust
pub fn load_api_key(config: ApiKeyConfig<'_>) -> Result<SecretString, LoadApiKeyError>;
pub fn load_setting(config: SettingConfig<'_>) -> Result<String, LoadSettingError>;
pub fn load_optional_setting(config: SettingConfig<'_>) -> Option<String>;
```

[Fact] `load_optional_setting(value: Option<String>, environment_variable: &str) -> Option<String>` needs no config struct because it emits no error. Config types are `ApiKeyConfig { api_key: Option<SecretString>, environment_variable, parameter_name, description }` and `SettingConfig { value: Option<String>, environment_variable, setting_name, description }`. Missing-key messages use `"{description} API key is missing. Pass it using the '{parameter}' parameter or the {ENV} environment variable."`. `settings::env_var(name)` alone reads process environment, with a local Clippy exception; non-UTF-8 values count as missing.

[Decision] Keys use `secrecy::SecretString` 0.10.3 and redacted `Debug`, exposing only during `Authorization` construction. Load inside request-time `headers()` closures, following the lazy `createOpenAI` `getHeaders` pattern, so missing environment variables do not fail `create_openai()`.

## 7. Provider option parsing

[Decision] Deserialize only `provider_options[provider]`; invalid data returns `InvalidArgument`, missing keys return `None`. Ignoring other providers' keys allows shared option sets.

```rust
pub fn parse_provider_options<T: DeserializeOwned + JsonSchema>(
    provider_key: &str,
    options: &ProviderOptions,
) -> Result<Option<T>, InvalidArgumentError>;
```

[Decision] The 2026-09-13 implementation requires only `T: DeserializeOwned`, without `JsonSchema`. Serde validates adapter-internal options; errors use `invalid {provider_key} provider options: {serde error}` and `InvalidArgumentError { argument: "provider_options", cause }`. Thus provider utilities need no `ferrin-schema` dependency ([Crate boundaries](02-crates.md), section 2).

## 8. Secure URLs

[Decision] Rules jointly prevent SSRF and DNS rebinding:

- Validate application/model download URLs before use: HTTP/HTTPS only (HTTPS by default, configurable), no embedded credentials, and reject hosts resolving to loopback, link-local, private, multicast, or reserved addresses.
- Follow redirects manually, revalidate each target, and cap hops.
- Resolve once and pin validated addresses for connection to prevent DNS rebinding.
- `trusted_origins`/`credentialed_origins` allow explicit exceptions or credentials for known origins.
- Abort downloads over 100 MiB; lints require outbound requests through audited entry points.

[Decision] `ferrin_provider_util::secure_url` implementation:

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

- Use `ipnet` 2.12.2 subnet tables: IPv4 `10/8`, `172.16/12`, `192.168/16`, `127/8`, `169.254/16`, `0/8`, `100.64/10`, `224/4`, `240/4`; IPv6 `::1`, `fc00::/7`, `fe80::/10`, and mapped `::ffff:0:0/96` checked as IPv4.
- Resolve with `tokio::net::lookup_host`, validate every result, and connect through reqwest `ClientBuilder::resolve_to_addrs`.
- `fetch` resolves `Location` to an absolute URL, revalidates, and strips `Authorization` on cross-origin redirects.
- Enforce cumulative size while streaming and abort immediately on excess.

[Fact] Implementation on 2026-09-13 (`crates/ferrin-provider-util/src/secure_url/`):

- `UrlPolicy` adds builder methods `allow_http()`, `allow_private_networks()`, `trust_origin(&Url)`, `credential_origin(&Url)`, `max_redirects(n)`, `max_body_bytes(n)`. `Scheme` is `Https | Http`; `ValidatedUrl { url, addresses: Vec<SocketAddr> }`.
- Validation order: scheme, embedded credentials, host presence, trusted-origin bypass without DNS, hostname rules (`localhost`, `*.localhost`, `*.local`, literals), DNS resolution, all-address subnet checks. `allow_private_networks` skips only subnet rejection, retaining resolution and pinning.
- IANA-based additions: IPv4 `192.0.0/24`, `192.0.2/24`, `198.18/15`, `198.51.100/24`, `203.0.113/24`; IPv6 `::`, `::1`, `fc00::/7`, `fe80::/10`, `fec0::/10`, `ff00::/8`, `2001:db8::/32`, `3fff::/20`. Mapped/compatible IPv4 (`::ffff:a.b.c.d`, `::a.b.c.d`), NAT64 (`64:ff9b::/96`, `64:ff9b:1::/48`), and 6to4 (`2002::/16`) also check embedded IPv4. `is_private_ip`/`is_private_hostname` are public.
- `fetch_with_headers(transport, url, headers, policy, cancellation)` strips `BLOCKED_DOWNLOAD_HEADERS` (hop-by-hop, proxy, cloud metadata, Cookie), sends `Authorization` only to credentialed origins, and defaults `User-Agent` to `ferrin/<version>`. Resolve 3xx `Location`, enforce hop limits, and retain only `User-Agent` and `Accept` across origins, preserving CDN content negotiation. Non-2xx responses return `Status`; `read_body` enforces `max_body_bytes`. `fetch` is the no-extra-headers convenience form.
- `Downloaded { url, data, media_type: Option<MediaType>, headers }` strips Content-Type parameters. `DownloadError { url: Box<Url>, kind: DownloadErrorKind }` uses `Validation(UrlValidationError) | Transport(TransportError) | Status { status, headers } | InvalidRedirect | TooManyRedirects { limit } | Cancelled`. Boxing the URL meets the 128-byte error threshold.

[Decision] Clippy disallows direct `reqwest::Client::get/post/execute` outside secure URL handling; see [Coding standards](../03-engineering/03-coding-standards.md).

## 9. ID generation

[Decision] IDs are `<prefix>-<random alphanumeric>`, default 16 random characters with `-`; core text/call IDs use 24. Prefixes identify sources in logs; 24 random characters make collisions negligible.

```rust
pub trait IdGenerator: Send + Sync { fn generate(&self) -> String; }
pub struct PrefixedIdGenerator { prefix: &'static str, size: usize }   // alphanumeric via rand 0.10 ThreadRng
```

Core prefixes: `ftxt` text parts, `call` invocations, `appr` approvals, `tool` fallback `tool` IDs. Tests use `ferrin_testing::SequentialIdGenerator`.

[Fact] `PrefixedIdGenerator::new(prefix: impl Into<String>, size)` stores an optional prefix; `unprefixed(size)` omits it and `with_separator(char)` replaces `-`. Alphabet `0-9A-Za-z`, source `rand::rng()`. `generate_id()` returns 16 unprefixed characters; `Fn() -> String + Send + Sync` automatically implements `IdGenerator`.

## 10. User-Agent

[Decision] `with_user_agent_suffix` appends space-separated products: application → `ferrin/<version>` → `ferrin-<provider>/<version>`, preserving caller identity under HTTP User-Agent syntax.

Ferrin chain: application → `ferrin/<core-version>` → `ferrin-<provider>/<provider-crate-version>`; agents prepend `ferrin-agent/tool-loop` to the SDK chain.

## 11. Verification items

- [Fact] (PV-015, `verification/pv015-reqwest`) reqwest 0.13.5 retains `resolve_to_addrs`, disabled redirect policy, and `https_only`, adding `http1_max_headers` (default 100) and `Error::is_dns()` per its changelog. Constructing 100 pinned clients took 5.8 ms, about 58 µs each. An unreachable pinned address timed out with `is_dns() == false`, confirming no further DNS lookup. Version 0.13 switches defaults to `rustls` (`rustls-tls` renamed `rustls`), aws-lc, and `rustls-platform-verifier`; `query`/`form` become optional; TLS methods are renamed with soft deprecations.
- [Decision] Dedicated clients per download target are viable at microsecond cost and request-bound memory lifetime; do not add direct hyper-util connections. Originally proposed reqwest features were `rustls`, `http2`, `stream`, `json`, `multipart`, `charset`; revised below on 2026-09-13.
- [Decision] Enable only workspace `rustls`, `http2`, and `stream`. Serialize JSON with `serde_json`, encode multipart locally, and decode response bytes as lossy UTF-8. Removing three features eliminates dependencies such as `mime_guess` and `encoding_rs` while ensuring test, recording, and reqwest transports see identical body bytes.
- [Fact] (PV-016, `verification/pv016-header-values`) `http` 1.5.0 `HeaderValue::from_str`/`from_bytes` accept 0x80–0xFF and tabs, reject newlines/DEL, and allow UTF-8 construction. `to_str()` rejects non-ASCII values; use `as_bytes()`.
- [Decision] `Headers::insert(&str, &str)` uses `HeaderValue::from_str`; read through ASCII-only `get_str()` or `get_bytes()`. Store metadata headers with lossy UTF-8 conversion. First-party request headers are ASCII and unaffected.

[Fact] 2026-09-15 retry parsing uses fallible duration conversion, ignoring non-finite, negative and out-of-range numeric delays; an invalid millisecond header still permits the seconds/date fallback (source: `crates/ferrin-provider-util/tests/suite/misc.rs`).

[Fact] 2026-09-15 SSE BOM detection completes before the first line is parsed, including a BOM split across chunks; regression tests enumerate every pair of chunk boundaries across BOM, CRLF and event separators (source: `crates/ferrin-provider-util/tests/suite/sse.rs`).
