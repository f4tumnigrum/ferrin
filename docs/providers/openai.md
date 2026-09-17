# OpenAI (`ferrin-openai`)

**English** | [Chinese](../zh-CN/providers/openai.md)

`ferrin-openai` is the L3 OpenAI adapter ([Crate boundaries](../01-architecture/02-crates.md)). This guide records capabilities, settings, options, metadata, limitations, and fixtures implemented on 2026-09-13; see the [implementation guide, section 9](../01-architecture/17-provider-implementation-guide.md#9-implementation-record-2026-09-13-ferrin-openai).

Provider IDs use `<name>.<family>`, default `name` `openai`, configurable through `OpenAiSettings::name`. Options/metadata default to the `openai` key, configured by provider_options_key.

## Capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| Responses generation/streaming | Implemented | `OpenAiResponsesLanguageModel`, `POST /responses`; default Provider language family. Source: `src/responses/`. |
| Chat Completions generation/streaming | Implemented | `OpenAiChatLanguageModel`, `POST /chat/completions`; `src/chat/`. |
| Completions generation/streaming | Implemented | `OpenAiCompletionLanguageModel`, `POST /completions`; `user:`/`assistant:` text prompts, no tools; `src/completion/`. |
| Tools | Implemented | Strict function tools and all `ToolChoice` variants. Provider IDs: `openai.web_search`, web_search_preview, file_search, code_interpreter, image_generation, mcp, tool_search, programmatic_tool_calling, apply_patch, local_shell, shell, computer, custom. `OpenAiTools` factories in `src/tools/mod.rs`. Chat/Completions accept function tools only. |
| Structured output | Implemented | Responses text.format json_schema (`strict` default `true`, configurable with `strictJsonSchema`) or `json_object`; Chat `response_format.json_schema`. normalize_json_schema warns on unsupported keywords. |
| Reasoning | Implemented | Nondefault `ReasoningEffort` maps to Responses `reasoning.effort` or Chat `reasoning_effort`. Infer o-series/GPT-5+ reasoning models except `chat` variants; `src/capabilities.rs`. Responses summary defaults `detailed`; GPT-6+ accepts `low`/`medium`/`high`/`xhigh`/`max` only. |
| Embeddings | Implemented | `POST /embeddings`, at most 2048 values, encoding_format float. |
| Images | Implemented | `POST /images/generations`; `files`/masks use multipart /images/edits. Dall-e-3 allows one per call, others ten; dall-e models explicitly request `b64_json`. |
| Speech | Implemented | `POST /audio/speech`; `mp3`/`opus`/`aac`/`flac`/`wav`/`pcm`, default `mp3` and `alloy` voice. |
| Transcription | Implemented | Multipart /audio/transcriptions; gpt-`realtime`-whisper models stream only through WebSockets, requiring `realtime`. |
| Speech translation | Implemented with `realtime` | WebSocket `/realtime/translations?model=<id>`. |
| Reranking | Unavailable | No corresponding OpenAI API; `NoSuchModelError`. |
| Video | Not implemented | `Provider::video_model` returns `NoSuchModelError`. |
| Files | Implemented | Multipart upload /files, GET metadata/content, delete. |
| Skills | Implemented | Multipart /skills with `files[]`. |
| Batches | Implemented | Upload JSONL files with purpose batch, then `POST /batches`; status/output and error JSONL streams/cancel/list. Text only, one model per batch, fixed `/v1/responses` endpoint and `24h` window. |
| Realtime | Implemented | Client secrets through `POST /realtime/client_secrets`; wss `realtime` model URL; config/event mappings, with core `realtime` driving connections. |

## Settings and environment variables

[Fact] `create_openai(OpenAiSettings)`, `src/lib.rs`:

| Setting | Environment | Behavior |
| --- | --- | --- |
| `base_url` | `OPENAI_BASE_URL` | Default `https://api.openai.com/v1`; trim trailing slashes; reject invalid URLs at construction. |
| `api_key` | `OPENAI_API_KEY` | Read on first request; missing keys fail that request with LoadApiKey. |
| `organization` / `project` | None | `openai-organization`/`openai-project` headers. |
| `headers` | None | Per-request additions, overridden by call `headers`. |
| `name` | None | ID prefix and file-reference key, default `openai`. |
| `transport` / `id_generator` | None | Shared `reqwest` `transport` and random IDs. |

Append `ferrin-openai/<crate-version>` to every User-Agent (`config::USER_AGENT`).

[Fact] Additional `OpenAiConfig` fields for compatible endpoints: `provider_options_key` (`openai`), `explicit_message_item_type` (`false`; adds message type), `supports_web_search_sources_include` (`true`), `file_id_prefixes` ([file-], recognizing text file IDs).

[Fact] Realtime defaults off and enables `tokio-tungstenite`, speech translation, and `realtime`-whisper transcription. Without it, speech translation returns an explanatory `NoSuchModelError`, supports_stream is `false`, and `do_stream` is unsupported.

[Fact] WebSockets authenticate with `realtime`/openai-insecure-api-key subprotocols, without Authorization; derive `ws`/`wss` from HTTP/HTTPS `base_url`.

## Provider options (provider_options["openai"])

[Decision] Keys are camelCase. Unknown option keys are ignored, matching reference Zod object parsing; malformed known fields still fail before HTTP. Complete schemas are the module ProviderOptions structs. Source: local AI SDK `6c6c221` option schemas.

[Fact] Responses options: `conversation`, `include`, `includeWebSearchSources`, `instructions`, `logprobs` (`true` or top-N), `maxToolCalls`, `metadata`, `parallelToolCalls`, `previousResponseId`, `promptCacheKey`, `promptCacheOptions {retention}`, `promptCacheRetention`, `reasoningEffort`, `reasoningEffortUpdate`, `reasoningSummary`, `reasoningMode`, `reasoningContext`, `safetyIdentifier`, `serviceTier`, `store`, `strictJsonSchema`, `systemMessageMode` (`system`/`developer`/`remove`), `textVerbosity`, `truncation`, `user`, `forceReasoning`, `contextManagement [{type, compactThreshold}]`, `compactionTrigger`, `passThroughUnsupportedFiles`. Part options: `itemId`, `reasoningEncryptedContent`, `phase`, `imageDetail`, `encryptedContent` for `openai.compaction`. Function tools: `deferLoading`, `allowedCallers`, `outputSchema`, `namespace`, `namespaceDescription`. Source: `src/responses/options.rs`.

[Fact] Chat options (`src/chat/options.rs`): `logitBias`, `logprobs`, `user`, `parallelToolCalls`, `maxCompletionTokens`, `store`, `metadata`, `prediction`, `reasoningEffort`, `serviceTier`, `promptCacheKey`, `promptCacheOptions`, `promptCacheRetention`, `safetyIdentifier`, `textVerbosity`, `strictJsonSchema`, `systemMessageMode`, `forceReasoning`.

[Fact] Completions: `echo`/`logitBias`/`logprobs`/`suffix`/`user`. Embeddings: `dimensions`/`user`. Images: `quality`/`style`/`background`/`moderation`/`outputFormat`/`outputCompression`/`inputFidelity`/`user`. Speech: `instructions`/`speed`. Transcription: `include`/`language`/`prompt`/`temperature`/`timestampGranularities`/`responseFormat`/`chunkingStrategy` (`auto` or server_vad with threshold/prefixPaddingMs/silenceDurationMs), `streaming {delay, include}`. Files: `purpose`/`expiresAfter {anchor, seconds}`. Batches: `inputFileExpiresAfter` seconds.

## Provider metadata (provider_metadata["openai"])

[Fact] Responses result fields: `responseId`/`serviceTier`/`reasoningContext`/`logprobs`. Text/reasoning: `itemId`/`reasoningEncryptedContent`/`phase`/`annotations`. Tool calls: `itemId`/`async`/`caller`/`namespace`. Provider results include `type`, `queries`/`query`, `sources`, `fileId`, `containerId`, `output`, `error`, `index` according to tool (`src/responses/output.rs` and stream/items.rs).

[Fact] Chat: `acceptedPredictionTokens`/`rejectedPredictionTokens`/`logprobs`; Completions: `logprobs`.

[Fact] Images: `images[{revisedPrompt}]`, `background`/`outputFormat`/`quality`/`size`/`created`, plus `imageTokens`/`textTokens` usage. Diarized transcription: segments with text/startSecond/endSecond/speaker. Files: `purpose`/`status`/`bytes`/`filename`/`createdAt`/`expiresAt`. Skills: `defaultVersion`/`createdAt`/`updatedAt`. Batches: `inputFileId`/`inputFileExpiresAt`.

[Decision] Usage mapping follows the local AI SDK `6c6c221` reference: absent Chat cache-read/reasoning counts become zero; Completion preserves absent totals, supplies zero-default uncached/text breakdowns and retains the raw usage object. Image input-token details divide across returned images with the remainder assigned to the last image, so their sum equals provider usage. Sources: reference `chat/convert-openai-chat-usage.ts`, `completion/convert-openai-completion-usage.ts`, `image/openai-image-model.ts`; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] File and batch resource IDs are encoded as individual path segments; blank file IDs fail before HTTP. File uploads accept numeric `expiresAfter` seconds as in the reference and retain the existing `{anchor, seconds}` form for compatibility; omitted response filenames fall back to the supplied filename, and unnamed multipart uploads use `blob`. Sources: reference `files/openai-files.ts`, `files/openai-files.test.ts`, `openai-batch.ts`; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] Existing tool factories validate their complete reference input/output schemas, including action variants, required fields, tuple sizes and defaults. Object inputs follow the reference parser: unknown fields are stripped unless an explicit record/passthrough policy preserves them; strict objects reject unknown fields. Provider configuration arguments are validated before request conversion. Sources: the local `6c6c221` tool schemas under `packages/openai/src/tool`, [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] Speech `speed` and `instructions` are sent only from the top-level speech options, matching the reference request builder. The duplicate `provider_options["openai"]` fields are parsed (including speed range 0.25–4.0) but do not override the request. Migrate calls using those duplicate fields to `.speed(...)` / `.instructions(...)` on the speech builder. Sources: reference `speech/openai-speech-model.ts::getArgs`, `speech/openai-speech-model-options.ts` and its top-level speed regression; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] `UploadData::Stream` uploads use streaming multipart through the shared transport, without collecting the file in memory before HTTP. Cancellation or dropping the request releases the input stream; source-stream failures use a redacted body error. Source: `src/files/mod.rs`, reference file upload implementation; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

[Decision] Responses custom tools use their registered tool name for declaration, choice and replay; the legacy `CustomToolArgs::name` hint is ignored. `async: true` on function/custom tools is omitted with a warning before GPT-6. Function `namespace` accepts the reference `{name, description}` object; legacy string plus `namespaceDescription` remains accepted, but conflicting namespace descriptions fail. Call-level `allowedTools {toolNames, mode}` overrides tool choice while preserving definitions, resolves provider aliases and warns/drops unsupported entries. Hosted shell skill references resolve the `openai` reference and default to version `latest`. Source: reference `responses/openai-responses-prepare-tools.ts`; [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), 2026-09-17.

## Known limitations and warnings

[Decision] Responses functions with `outputSchema` normalize that schema using the same `propertyNames` rules as input schemas. Their text, error-text and denied results are encoded as JSON string literals, while JSON results retain their existing JSON encoding. This lets OpenAI parse the result string against `output_schema`. Source: local AI SDK `6c6c221`, `prepareFunctionTool` and `convertFunctionToolResultOutput`; local request regression coverage only.

- [Fact] Responses drops `topK`/`seed`/`presencePenalty`/`frequencyPenalty`/`stopSequences` with warnings. Reasoning models drop `temperature`/`topP` except supported sampling with effort none; GPT-6+ drops `logprobs`. Nonreasoning models warn on reasoning options. Flex requires o3/o4-mini/GPT-5+; `priority`/`fast` requires gpt-4*, GPT-5 except `nano`/`chat`, or o3+, otherwise omitted with warnings.
- [Fact] Responses images accept URLs/bytes/file IDs. Other files accept PDF bytes or arbitrary URLs; unsupported types fail unless passThroughUnsupportedFiles. Text file data is an ID only when matching `file_id_prefixes`.
- [Fact] Chat lacks `topK`. Reasoning models drop sampling/`logprobs`/penalties/`logitBias` and use `max_completion_tokens`; search-preview drops `temperature`. Files accept images, wav/mp3/mpeg audio, and PDF; text types are unsupported.
- [Fact] Completions warns for `tools`/`toolChoice`/`responseFormat`/`topK`; tool messages/calls are unsupported and noninitial system messages are `InvalidPrompt`.
- [Fact] Speech falls back to `mp3` for unsupported formats with warnings; `language` is unsupported. Realtime-whisper is streaming-only, other transcription models non-streaming-only. Diarize defaults auto chunking and `diarized_json`.
- [Decision] Stream errors terminate after closing open parts, without a later `Finish`; see implementation guide section 9.
- [Decision] Pre-output errors fail `do_stream` as ApiCall: `rate_limit`/`quota` 429, `authentication` 401, `permission` 403, `not_found` 404, `invalid_request` 400, `overloaded` 503, `timeout` 504, otherwise 500. This enables request retries.
- [Fact] Batch results require terminal state; running batches return `InvalidArgument` and image requests are unsupported.
- [Fact] References must use provider `name`; other keys return `NoSuchProviderReference`.
- [Fact] Invalid base64 realtime audio returns `InvalidResponseData`.

## Fixture inventory

Fixtures under `crates/providers/ferrin-openai/tests/fixtures/<area>` are replayed by suite tests through FixtureServer; streaming cases use `-stream` suffixes.

| Area | Cases | Coverage |
| --- | --- | --- |
| `responses` | `text-basic`, `tool-call`, `reasoning`, `error-400`, `error-429`, `text-basic-stream`, `tool-call-stream`, `reasoning-stream`, `error-early`, `error-late` | Text/tools/`reasoning` summaries and encrypted content, usage, errors, contracts, early/late failures |
| `chat` | `text-basic` with `url_citation`, `tool-call`, `error-401`, `text-basic-stream`, `tool-call-stream`, `error-early` | Text/sources/tool deltas/usage/prediction metadata/errors |
| `completion` | `text-basic`, `text-basic-stream` | Text/usage |
| `embedding` | `basic` | Vectors/usage |
| `image` | `generate` | Base64/metadata/usage |
| `transcription` | `verbose`, `diarized`, `words` | Segments/language/speakers/multipart |
| `files` | `upload`, `get`, `delete` | Upload/metadata/download/`delete` |
| `skills` | `upload` | Multipart files and mapping |
| `batch` | `file-upload`, `create`, `retrieve-pending`, `retrieve-completed`, `retrieve-failed`, `cancel`, `list`, `output.jsonl`, `errors.jsonl` | JSONL/state/results/cancellation/paging |
| `realtime` | `client-secret` | Temporary secrets/session config |

[Pending verification] (PV-031) The original fixtures were handwritten from official response schemas. Four additional Responses cases were recorded through a third-party proxy on 2026-09-17 (below); official OpenAI responses and the remaining cases still need recording and comparison.

## Recorded proxy verification (2026-09-17)

[Fact] `responses/recorded-proxy/` contains four credentialed recordings from a private third-party Responses endpoint, requested and reported model `gpt-5.6-sol`, with scenario, request, response/SSE and metadata files. Final recordings were captured at 08:11:35–08:12:08 UTC using `store: false`, reasoning effort `low` and `max_output_tokens: 512`. Each returned HTTP 200. The provider identity and endpoint are intentionally omitted; scenarios read the endpoint from `OPENAI_BASE_URL`. Source: fixture metadata and `tests/suite/responses_recorded.rs`.

[Fact] Replay compares the SDK-generated request body with each recorded request, checks full upstream `Usage.raw`, snapshots normalized outputs/usage, and checks the SSE contract and normalized event sequence. Comparison results from the four recordings:

| Case | Output | Input / output tokens | Cached input | Finish |
| --- | --- | --- | --- | --- |
| `text-basic` | `pong` | 4393 / 5 | 4224 | `stop` |
| `text-basic-stream` | `pong` | 4393 / 5 | 4224 | `stop` |
| `tool-call` | `get_weather({"city":"Berlin"})` | 4432 / 18 | 0 | `tool-calls` |
| `structured-output` | `{"city":"Paris","country":"France"}` | 4426 / 16 | 0 | `stop` |

[Fact] Compared with the original hand-authored text/tool fixtures, these responses contain additional `usage.attribution` and `cache_write_tokens` fields, proxy-injected `instructions`, account/cache identifiers and output metadata. The recorder replaces `instructions`, `safety_identifier` and `prompt_cache_key` at the response root and SSE `response` root with `[REDACTED]`; each metadata file lists these pointers. Output, usage, event order and request/response IDs are retained. The original synthetic fixtures remain useful for reasoning/error branches absent from these recordings.

[Fact] The SSE recording contains nine events, from `response.created` through `response.completed`, with one `response.output_text.delta`; its terminal `output` omits item IDs present in earlier events. Ferrin still produces a complete text stream and terminal `Finish`. The JSON and SSE responses report `max_output_tokens: null` despite requesting 512, and attribute 4382 input tokens to injected instructions. These observations do not establish that the requested limit is enforced or establish the proxy's actual charges. Source: recorded request/response pairs and normalized snapshots.

[Fact] Seven existing Responses live tests passed on 2026-09-17 with the same endpoint/model and `{"openai":{"store":false,"reasoningEffort":"low"}}`: four facade tests (text, streaming, local tool execution plus result round trip, typed structured output) and three adapter tests (text, streaming, tool-call output). Source: nextest run `dcf4adb2-6a4a-4dbd-a979-9260998c7a56`; this run excludes Chat Completions. Reproduce after exporting the endpoint/model/key and provider options:

```sh
cargo xtask record-fixture --provider openai --case responses/recorded-proxy/text-basic
INSTA_UPDATE=no just test -E "'test(responses_recorded)'"
just test --run-ignored only -E "'(package(ferrin) | package(ferrin-openai)) & test(live_) & !test(live_chat_)'" --test-threads 1
```

[Fact] Verification is limited to this proxy and model alias. It does not verify the official OpenAI endpoint, other providers, streamed tool arguments, streamed structured output, reasoning output, errors, other modalities or original transport byte boundaries (the recorder stores SSE event boundaries). Redacted values are not covered by replay. PV-031 remains open.

## Additional implementation records

[Decision] Responses advanced-tool roundtrips follow [ADR 0022](../04-decisions/2026-09-17-0022-provider-tool-roundtrips.md): hosted program, search and shell items retain their IDs and replay fields; client search outputs retain their protocol type; provider caller bindings permit deferred program results. Undeclared parallel wrappers expand only for declared function recipients and preserve ordered replay metadata. Verification is deterministic and does not close PV-031.

[Fact] Live verification on 2026-09-14: default `store` true sends previous assistant/provider-tool items as `item_reference`. A third-party proxy returned 502 for references but accepted full items. Use {"openai":{"`store`":false}} for endpoints not storing items; convert_prompt then sends complete content. This run did not test the official OpenAI endpoint.

[Decision] A configured Responses `conversation` does not imply that local tool results have been uploaded: tool messages always send their function/custom/provider-defined outputs. Source: `responses/convert_tool_results.rs`; regression `conversation_sends_new_local_tool_results` (2026-09-15).

[Decision] Each `openai.custom` tool maps its application alias to `args.name` independently for calls, results, forced choice, and response names; the tool type `custom` is not a function name. Source: `responses/convert_tools.rs`; regression `custom_tool_aliases_roundtrip_calls_results_and_choice` (2026-09-15).

[Fact] Local `openai.shell` results use `shell_call_output` with an `output` array of stdout/stderr/outcome entries, converting `outcome.exitCode` to `exit_code`; legacy `openai.local_shell` retains `local_shell_call_output`. Source: `responses/convert_tool_results.rs`; regression `local_shell_outputs_use_the_matching_api_generation` (2026-09-15).

[Fact] Responses and Chat request preparation resolve uploaded file references with `OpenAiConfig.name`, independently of the provider options key. Standalone prompt-conversion helpers retain the default `openai` name. Source: regression `uploaded_files_roundtrip_with_custom_provider_name` (2026-09-15).

[Decision] Provider tool argument conversion renames only documented API fields and traverses known configuration objects; headers, metadata, schemas, and unknown argument values remain opaque. This preserves user dictionary keys and HTTP header spelling. Source: `responses/convert_tools.rs`; regression `provider_tool_options_preserve_opaque_dictionary_keys` (2026-09-15).

[Decision] SSE EOF is successful only after an explicit provider terminal response or finish reason (including a Google prompt block). Earlier EOF emits `InvalidResponseData`, closes open parts through the stream driver, and never flushes incomplete tool arguments into executable calls. Source: stream EOF fixture-boundary regressions (2026-09-15); no live API verification.

[Fact] DALL-E image edits send a single multipart `image` and explicitly request `response_format=b64_json`; GPT image edits use `image[]` and the default base64 response. Multiple DALL-E input images are rejected before the request. Source: `image/mod.rs` and `image_edits_use_model_specific_file_fields_and_response_format` (2026-09-15).

[Fact] Chat generation and streaming retain the complete upstream `usage` object in `Usage.raw`, including audio counters and fields not modeled by normalized usage. Source: `chat/mod.rs`, `chat/stream.rs`; regression `raw_usage_preserves_unmodeled_fields_in_generate_and_stream` (2026-09-15).

[Decision] Responses/Chat preserve supplied function and structured-output schemas after `propertyNames` compatibility normalization. Function `strict` is sent only when explicitly supplied; `strictJsonSchema` controls only the structured-output strict flag (default `true`). Under [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md), this replaces the automatic strict schema transforms recorded on 2026-09-15. Applications can still explicitly use the fallible transform in [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md). Sources: local AI SDK `6c6c221` Responses/Chat request encoders and `strict_schema` regressions; request-shape verification only.

[Fact] Advanced-tool regressions (`responses_advanced.rs`, `responses_parallel.rs`, `tools.rs`, 2026-09-17) cover generation/stream equivalence, hosted shell classification, client search results, program caller identity, no-storage replay, ordered complete parallel groups and batch mapping. Batch results lack request function declarations, so internal parallel wrappers remain unexpanded there. Programmatic execution-denied results are rejected before HTTP. These are synthetic fixture checks, not live provider verification.

[Fact] Hosted shell environment kinds map `containerAuto`/`containerReference` to `container_auto`/`container_reference`; network-policy keys are converted while secret entries remain opaque. Source: `hosted_shell_environment_uses_wire_types_and_preserves_secret_names` (2026-09-17), request-shape test only.
