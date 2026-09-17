# Anthropic (`ferrin-anthropic`)

**English** | [Chinese](../zh-CN/providers/anthropic.md)

`ferrin-anthropic` is the L3 Anthropic adapter ([Crates](../01-architecture/02-crates.md)). This guide records the 2026-09-13 implementation; see [implementation guide section 10](../01-architecture/17-provider-implementation-guide.md#10-implementation-record-2026-09-13-ferrin-anthropic).

IDs use `<name>.<family>`, default `anthropic`. Read standard options then custom-`name` overrides; write result metadata to both keys when different.

## Capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| Messages generation/streaming | Implemented | `AnthropicMessagesLanguageModel`, `POST /messages`; messages/`chat` alias and default Provider family. Sources: messages.rs, request/, output/, stream.rs. |
| Tools | Implemented | Function `strict`/`cacheControl`/`deferLoading`/`allowedCallers`/`eagerInputStreaming`/`input_examples`, all choices. Provider bash/computer/text_editor families, memory_20250818, web_search/web_fetch/code_execution families, tool_search_regex_20251119, tool_search_bm25_20251119, advisor_20260301. Factories in tools.rs, wire mapping in prepare_tools.rs. |
| Structured output | Implemented | Native `output_config.format` on capable models with auto/`outputFormat`, sanitized schema; otherwise jsonTool fallback with any choice and parallel disabled. Synthetic calls become text and `tool_use` finish becomes `stop`. |
| Reasoning | Implemented | Adaptive models use adaptive/summarized `thinking` and `output_config.effort`, downgrading unsupported `xhigh` to `max` with warning. Others use token budgets; None disables `thinking`. Thinking/redacted blocks become reasoning parts. |
| Citations | Implemented | Document and web search/fetch citations become document/URL sources; retain raw web citations in text metadata. |
| Embeddings/images/speech/transcription/reranking/video/realtime | Unavailable | No corresponding API; embedding/image return `NoSuchModelError`, others use `Provider` defaults. |
| Files | Partial | Multipart upload with `files-api-2025-04-14`; metadata/download/delete unsupported. |
| Skills | Implemented | Multipart /skills with `files[]` and `skills-2025-10-02`; `latest_version` triggers a version GET for name/description. |
| Batches | Implemented | Messages batches, status, `results_url` JSONL, cancel, list with `limit`/`after_id`; text only. |

## Settings and environment variables

[Fact] `create_anthropic(AnthropicSettings)`, `src/lib.rs`:

| Setting | Environment | Behavior |
| --- | --- | --- |
| `base_url` | `ANTHROPIC_BASE_URL` | Default `https://api.anthropic.com/v1`; add `/v1` to bare origins, trim slashes, reject invalid URLs immediately. |
| `api_key` | `ANTHROPIC_API_KEY` | Lazy `x-api-key`; missing credentials return LoadApiKey. |
| `auth_token` | `ANTHROPIC_AUTH_TOKEN` | Bearer auth; explicit key and token together are invalid. Environment token applies only without key. |
| `headers` | None | Call `headers` override; merge beta flags from both and request inference. |
| `name` | None | ID prefix, extra options key, file/skill references; default `anthropic`. |
| `transport` / `id_generator` | None | Shared `reqwest` and random source IDs. |

[Fact] Send anthropic-version 2023-06-01 and append `ferrin-anthropic/<version>`. Beta headers are lowercased/deduplicated/sorted/comma-joined and omitted when empty.

[Fact] `supports_strict_tools` and `supports_native_structured_output` default `true`; disabling them warns/ignores `strict` or forces `json`-tool fallback respectively.

## Provider options (provider_options["anthropic"])

[Decision] CamelCase keys; unknown keys are ignored, matching reference Zod object parsing, while invalid known fields and enum values return InvalidArgument. See `src/options.rs` for schemas. Source: local AI SDK `6c6c221`, `anthropic-language-model-options.ts`.

[Fact] Model options: `sendReasoning`; `structuredOutputMode` `outputFormat`/`jsonTool`/`auto`; thinking type adaptive/enabled/disabled, budgetTokens, display omitted/summarized/updates, blockBinding prefixMismatchBehavior error/drop_block; `disableParallelToolUse`; cacheControl ephemeral with ttl 5m/1h; metadata userId; mcpServers URL/name/auth/toolConfiguration enabled/allowedTools; container id/skills with anthropic skillId or custom providerReference and version; `toolStreaming`; `effort` `low`/`medium`/`high`/`xhigh`/`max`; taskBudget tokens with total ≥20000 and remaining; `speed` `fast`/`standard`; `serviceTier` `auto`/`standard_only`; `inferenceGeo` `us`/`global`; `fallbacks` default or model array; anthropicBeta; contextManagement edits clear_tool_uses_20250919/clear_thinking_20251015/compact_20260112.

[Fact] Part/message options: files `containerUpload`/citations.enabled/`title`/`context`; system `clearAt`/`effort`/toolChanges type/toolName, inline noninitial systems with mid-conversation beta flags; reasoning `signature`/`redactedData`; calls `caller`/type mcp-tool-use/`serverName`; `cacheControl` at message/part levels, maximum four breakpoints, warning on excess/unsupported positions. Function options: `cacheControl`/`deferLoading`/`allowedCallers`/`eagerInputStreaming`. Batch-level `anthropicBeta`.

[Fact] Beta mappings: MCP servers → `mcp-client-2025-04-04`; `container` → `code-execution-2025-08-25`, `skills-2025-10-02`, `files-api-2025-04-14`; context management → `context-management-2025-06-27` plus `compact-2026-01-12` when relevant; `taskBudget` → `task-budgets-2026-03-13`; fast → `fast-mode-2026-02-01`; thinking updates → `thinking-display-updates-2026-08-18`; binding → `thinking-binding-controls-2026-08-01`; default/model-array fallback → `server-side-fallback-2026-07-01`/2026-06-01; PDF → `pdfs-2024-09-25`; references → files API; `strict` or structured-capable function tools → `structured-outputs-2025-11-13`; callers/examples → `advanced-tool-use-2025-11-20`. Provider tool beta mapping is in prepare_tools.rs.

## Provider metadata (provider_metadata["anthropic"])

[Fact] Result fields: raw `usage`, `stopSequence`, `stopDetails`, `inputTransformations`, `iterations` (`null` if missing), container id/expiresAt/skills (`null` if missing), contextManagement appliedEdits (`null` if missing). Copy to custom-`name` metadata when configured.

[Fact] Parts: raw web `citations`, reasoning signatures/`redactedData`, caller type/toolId, MCP type/serverName, source `citedText`/`encryptedIndex`/page or character ranges/`pageAge`. Compaction becomes text with compaction metadata; container uploads become `anthropic.container_upload` custom parts.

[Fact] Input total sums ordinary/cache-write/cache-read tokens; no_cache is input_tokens; reasoning comes from thinking_tokens. Iteration usage includes compaction, excludes advisor, and substitutes fallback rounds. Batch metadata: `requestCounts`/`archivedAt`/`cancelInitiatedAt`/`endedAt`/`resultsUrl`, failed `requestId`. Files: `filename`/`mimeType`/`sizeBytes`/`createdAt`/`downloadable`. Skills: `source`/`createdAt`/`updatedAt`.

[Decision] Existing tool factories validate their complete reference input/output schemas, including action variants, required fields, tuple sizes and defaults. Object inputs follow the reference parser: unknown fields are stripped unless an explicit record/passthrough policy preserves them; strict objects reject unknown fields. Provider configuration arguments are validated before request conversion. Sources: the local `6c6c221` tool schemas under `packages/anthropic/src/tool`, [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] `UploadData::Stream` uploads use streaming multipart through the shared transport, without collecting the file in memory before HTTP. Cancellation or dropping the request releases the input stream; source-stream failures use a redacted body error. Source: `src/files.rs`, reference file upload implementation; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

## Known limitations and warnings

[Decision] `code_execution_20250825` and `code_execution_20260120` bind provider callers and support deferred results, preserving existing `allowedCallers` entries when adding their own type. Source: [ADR 0022](../04-decisions/2026-09-17-0022-provider-tool-roundtrips.md); deterministic caller-preparation regression, with live API verification still covered by PV-031.

- [Fact] Drop penalties/`seed` with warnings; clamp `temperature` to 0–1, drop `topP` when `temperature` also set, and drop `temperature`/`topK`/`topP` for thinking or nonsampling models.
- [Fact] Output defaults: Sonnet 4.x/Haiku 4.5 64000, Opus 4.x 32000, Sonnet/Opus 4.6+ 128000, Claude 3 Haiku/older 4096. Add thinking budget then clip with warning. Unknown claude IDs use latest capabilities with warnings; non-Claude IDs use 4096/no structured output.
- [Fact] Images accept bytes/URL/references; PDF/plain text also accept inline text; others unsupported. Reference keys must match `name`; containerUpload requires references. Assistant files/reasoning files/custom parts warn and are skipped.
- [Fact] sendReasoning false or missing `signature`/`redactedData` drops reasoning with warnings. Trim trailing text whitespace in the final assistant message.
- [Fact] Denials send is_error true with `Tool call execution denied.` Map provider calls/results to server/mcp tool blocks by kind; warn on unknown kinds.
- [Fact] New web-search/fetch tools used with code execution mark calls dynamic; skills without code execution warn that it is required.
- [Decision] Stream errors close open parts and terminate without `Finish`; see implementation guide section 9.
- [Decision] Early errors fail `do_stream` as ApiCall: `api_error` 500, overloaded 529, rate_limit 429 (retryable); `request_too_large` 413, authentication 401, permission 403, not_found 404, billing/invalid_request 400, otherwise 500. HTTP responses retain server status.
- [Fact] Batch IDs match `^[A-Za-z0-9_-]{1,64}$` and are unique. Images, per-request beta, `speed`, fallback `speed`, renamed provider tools, and JSON-tool fallback are unsupported; webhooks warn. Running/archived results are invalid; missing `results_url` is `InvalidResponseData`. Cross-origin results URLs receive no credentials.
- [Fact] Different `message_start` IDs within one stream cause terminal `InvalidResponseData`.

## Fixture inventory

Fixtures under `crates/providers/ferrin-anthropic/tests/fixtures/<area>` replay through FixtureServer; streaming cases use `-stream` with event/data lines.

| Area | Cases | Coverage |
| --- | --- | --- |
| `messages` | `text-basic`, `tool-call`, `reasoning`, `web-search`, `citations`, `json-tool`, `error-400`, `error-429`, `text-basic-stream`, `tool-call-stream`, `reasoning-stream`, `code-execution-stream`, `json-tool-stream`, `error-early-stream`, `error-late-stream` | Text/cache/tools/thinking/signatures/web/document `citations`/JSON fallback/errors/contracts/code execution/container metadata |
| `files` | `upload` | Multipart/beta/metadata |
| `skills` | `upload`, `upload-no-version`, `version` | Multipart/`version` lookup/metadata |
| `batch` | `create`, `status-in-progress`, `cancel`, `list`, `results.jsonl` | Requests/state/counts/success/error/`cancel`/expiry/unknown results/paging |

Insta snapshots in tests/suite/snapshots cover requests, prompts, tool wire shapes, and stream parts; 58 tests.

[Pending verification] (PV-031) Handwritten official-schema fixtures still need real-response recording.

[Fact] `thinking.blockBinding.prefixMismatchBehavior` uses camelCase option spelling and becomes `thinking.block_binding.prefix_mismatch_behavior` on the wire; the legacy snake_case option remains accepted. Regression: `tests/suite/messages_request.rs::documented_block_binding_options_use_camel_case` (2026-09-15).

[Fact] Structured-output sanitization preserves `$ref` siblings, including `$id`, `$defs` and `definitions`, and sanitizes definitions without expanding recursive references. Regression: `tests/suite/unit.rs::schema_references_retain_definitions_and_scope` (2026-09-15).

[Fact] Provider-defined tool aliases map to provider names in forced tool choices and back to registered names in non-streaming, streamed and prefilled tool calls. Regression: `tests/suite/tools.rs::provider_tool_aliases_roundtrip_through_choices_and_calls` (2026-09-15).

[Fact] The four cache-breakpoint limit is shared by all prompt parts and tool definitions in one request; excess breakpoints are removed with warnings. Regression: `tests/suite/messages_request.rs::cache_breakpoint_limit_is_shared_across_prompt_and_tools` (2026-09-15).

[Fact] Batch result URLs are validated using `url_policy`: HTTPS/public addresses by default, resolved addresses pinned, redirects rejected, and streaming response bytes bounded by `max_body_bytes`. Credentials and caller headers are sent only to the configured origin or explicit `credentialed_origins`. Local test endpoints require explicit `allow_http().trust_origin(...)`. Sources: `src/batch/`, `tests/suite/security.rs` (2026-09-15).

[Fact] Modern code-execution factories validate programmatic, bash and text-editor inputs plus their result variants; the 20260120 factory also accepts encrypted execution output. Source: `src/tools/code_execution.rs` and `tests/suite/tools.rs` (2026-09-17); deterministic schema/caller tests only.
