# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- Redact live HTTP session and event IDs from transport debug output.

- Release OAuth flow coordination when an authentication future is cancelled.

- Resolve HTTP SSE requests with their own error on EOF or decoding failure before a matching response.

- Release request SSE bodies when either the request or connection is cancelled.

- Apply MCP request deadlines to sending and input rounds; cancellation cleanup never blocks timeout delivery. Failed-connect cleanup has a separate one-second bound and preserves the original error.

- Release client and built-in transport tasks when their final public handle is dropped.

## [0.1.0] - 2026-09-14

### Changed

- MCP tools built from server-provided schemas are marked `strict = false`
  so providers with strict-by-default function tools (OpenAI Responses)
  accept schemas that do not list every property as required.
- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

### Added

- `protocol`: JSON-RPC 2.0 message types (`JsonRpcMessage`, `RequestId`,
  error codes), MCP method params/results with forward-compatible `extra`
  fields, protocol version constants, `ProtocolEra` and `_meta` keys.
- `transport`: `McpTransport` trait, `TransportConfig`, `HttpTransport`
  (Streamable HTTP: POST with JSON or SSE responses, legacy sessions and GET
  inbound stream with `last-event-id` reconnection, `DELETE` termination,
  redirect modes, 401 handling), legacy `SseTransport`, `StdioTransport`
  (feature `stdio`, environment allow-list, single frame-writer task) and the
  `x-mcp-header` binding helpers (`Mcp-Param-*`).
- `client`: `McpClient`/`McpClientConfig` with `server/discover` negotiation
  and `initialize` fallback, `_meta` injection, `resultType`/`input_required`
  multi-round-trip handling through `ElicitationHandler`, request timeouts and
  cancellation with `notifications/cancelled`, capability assertions, tool-call
  retries, server `ping`/`elicitation/create` handling, notification and
  uncaught-error hooks; methods for tools, resources, resource templates,
  prompts, completion, `ping` and `logging/setLevel`.
- `tools`: `McpClient::tools` / `tools_from_definitions`, `ToolsOptions`,
  `ToolSchemas::{Automatic, Explicit}`, `ToolSchemaPair`, `McpToolExecutor`
  and `mcp_to_model_output`.
- `apps`: MCP Apps helpers (`app_tool_meta`, `split_app_tools`,
  `read_app_resource`, `fingerprint_app_resource`, `detect_app_resource_drift`).
- `oauth` (feature `oauth`): `OAuthClientProvider`, protected resource and
  authorization server metadata discovery, dynamic client registration, PKCE
  `S256` authorization, code exchange and refresh, `auth` with credential
  invalidation retries, `WWW-Authenticate` parsing.
- `McpError` with `TransportFailure` details and `is_retryable_tool_call`.
