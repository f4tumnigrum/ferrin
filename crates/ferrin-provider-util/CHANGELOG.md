# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `http` module: `HttpTransport` trait, `HttpRequest` (method, URL, headers,
  `RequestBody::{Empty, Bytes, Multipart}`, cancellation token, timeout,
  pinned addresses), `HttpResponse` with a streaming body, `TransportError`
  with `TransportErrorKind` and retryability, `MultipartForm` encoder,
  `read_body` with a 2 GiB default limit.
- `ReqwestTransport` (feature `reqwest`): rustls, HTTP/2 via ALPN, no automatic
  redirects, no default timeout, per-request DNS pinning through
  `resolve_to_addrs`, cancellation of the request and of the body stream;
  `default_transport()` shared instance.
- Request helpers `post_json`, `post_form`, `post_bytes`, `get`, `delete`,
  `send` returning `ApiResponse { value, response_headers, raw }`.
- Response handlers: JSON, text, JSON error (message extractor and
  retryability override), status code, binary, binary stream, server-sent
  events (`[DONE]` skipped) and JSON Lines, the last two yielding
  `ParseResult` items so parse failures do not end the stream.
- `sse::SseDecoder` and `sse::decode_stream`: WHATWG fields, multi-line data,
  comments, `\r\n`/`\r`/`\n`, BOM, per-event size limit, receive timestamps.
- `secure_url`: `UrlPolicy`, `Scheme`, `validate_url` (scheme, credentials,
  private hostname and address checks over IPv4/IPv6 tables including
  embedded-IPv4 forms, DNS resolution), `fetch`/`fetch_with_headers` with
  manual validated redirects, cross-origin header stripping, credentialed
  origins, body limit; `DownloadError` with `DownloadErrorKind`.
- `settings`: `load_api_key`, `load_setting`, `load_optional_setting`,
  `env_var`.
- `ids`: `IdGenerator`, `PrefixedIdGenerator`, `generate_id`.
- `media_type`: `detect_media_type`, `detect_media_type_for`,
  `detect_media_type_base64`, `media_type_to_extension`,
  `resolve_full_media_type`.
- `reasoning`: `map_reasoning_to_effort`, `map_reasoning_to_budget`,
  `BudgetPercentages`, `is_custom_reasoning`.
- `tool_name_mapping::ToolNameMapping`, `streaming_tool_call::StreamingToolCallTracker`,
  `provider_options::parse_provider_options`,
  `provider_reference::resolve_provider_reference`,
  `response_metadata::response_metadata`, `batch::normalize_batch_request_counts`,
  `base_url::{parse_base_url, without_trailing_slash, join_path}`,
  `headers::{sanitize_download_headers, is_same_origin, strip_to_public_headers}`,
  `retry::{is_retryable_status, retry_after, retry_after_within}`.
- `stream_driver`: the `StreamMachine` trait, `drive_stream` (closes open
  text, reasoning and tool-input parts before a terminal `StreamPart::Error`
  and emits no `Finish` after it) and `fail_on_early_error` (turns an error
  frame received before any output into a request failure), shared by the
  streaming language models of the provider crates.

### Changed

- reqwest is enabled with only the `http2` and `stream` features; multipart
  bodies are encoded by this crate and JSON goes through `serde_json`.
- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
