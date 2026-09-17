# Google Generative AI (`ferrin-google`)

**English** | [Chinese](../zh-CN/providers/google.md)

`ferrin-google` is the L3 Google Generative AI/Gemini adapter ([Crates](../01-architecture/02-crates.md)). This guide records the original 2026-09-14 implementation and the 2026-09-17 Interactions/Live audio additions; see [implementation guide section 12](../01-architecture/17-provider-implementation-guide.md#12-implementation-record-2026-09-14-ferrin-google).

IDs use `name`.generative-ai/speech/transcription/batch/realtime; embedding/image/video/files use bare `name`. Default `google` is configurable. Read `google` then custom-`name` options; write result metadata to both distinct keys.

## Capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| `generateContent` / `streamGenerateContent` | Implemented | `GoogleLanguageModel`, POST models/{model}:`generateContent` or `streamGenerateContent`?alt=sse; language_model/`chat` alias. Sources: language_model/request/convert_prompt/output/stream/json_accumulator. |
| Tools | Implemented | Convert function schemas to OpenAPI subset, falling back to `parametersJsonSchema` for recursive refs. Choices map to `AUTO`/`VALIDATED`/`ANY`/`NONE` and `allowedFunctionNames`. Tools: google_search, enterprise_web_search, url_context, code_execution, file_search, vertex_rag_store, google_maps under google prefix; `GoogleTools`/prepare_tools. |
| Structured output | Implemented | JSON responseMimeType plus converted `responseSchema` unless `structuredOutputs` is `false`. |
| Reasoning | Implemented | Gemini 3+ thinkingLevel: `minimal`/`None` → lowest, `low`/`medium`/`high`, `xhigh` → `high`. Gemini 2.5 uses token budget capped at 32768 Pro/24576 others; `None` →0. Thought parts become reasoning and retain signatures. |
| Citations | Implemented | Grounding `web`/`image`/`retrievedContext`/`maps` become URL/document sources; streaming deduplicates URLs. |
| Embeddings | Implemented | embedContent or batchEmbedContents, maximum 100 values; dimensions/`taskType`/multimodal `content`. |
| Images | Implemented | Gemini `generateContent` IMAGE modality, aspect/image config, references, search; see `size`/`mask`/n limits. |
| Speech | Implemented | `AUDIO` generation, default `Kore`, WAV or raw PCM, multi-speaker configuration. |
| Transcription | Implemented | Single Interactions `POST /interactions`, `word_info` segments; `realtime` enables Live `do_stream` for `-live` IDs. |
| Video | Implemented | predictLongRunning start/status with first/last/reference images, aspect/`resolution`/duration/`seed`; direct generation unsupported. |
| Files | Partial | Two-request resumable upload and polling to `ACTIVE`, metadata/delete; no download. |
| Batches | Implemented | batchGenerateContent inline or JSONL upload at ≥20 MB, status, inline/file JSONL results, cancel/list; text and images share image-to-language conversion. |
| Realtime Live API | Implemented | Temporary auth tokens, WebSocket `setup` and bidirectional event mapping. |
| Interactions language model | Implemented | `interactions(model_id)`; unary/SSE, stateful replay, background polling and resumable streaming. |
| Speech translation | Implemented with `realtime` | `speech_translation(model_id)`, target-language text and 24 kHz PCM output. |
| Reranking | Unavailable | No Gemini endpoint; `Provider` defaults. |

## Settings and environment variables

[Fact] `create_google(GoogleSettings)`, `src/lib.rs`:

| Setting | Environment | Behavior |
| --- | --- | --- |
| `base_url` | None | Default `https://generativelanguage.googleapis.com/v1beta`; trim slashes and reject invalid URLs. Upload/download/token paths use origin. WebSockets remove trailing `v1beta`/`v1alpha`, append `/ws/<service>`, and convert HTTP schemes to `ws`/`wss`. |
| `api_key` | `GOOGLE_GENERATIVE_AI_API_KEY` | Lazy `x-goog-api-key`; missing keys fail with LoadApiKey. |
| `headers` | None | Per-request additions with call overrides. |
| `name` | None | Provider prefix/options/reference key, default `google`. |
| `transport` / `id_generator` | None | Shared `reqwest` and random fallback call/source/batch IDs. |

[Fact] Append `ferrin-google/<version>` User-Agent. The second resumable upload request uses URL authorization without `x-goog-api-key`; temporary tokens use query `key` instead.

## Provider options (provider_options["google"])

CamelCase keys; unknown keys return InvalidArgument. Schemas are in `src/options.rs`.

[Fact] Language options: responseModalities TEXT/IMAGE; thinkingConfig thinkingBudget/includeThoughts/thinkingLevel merged with inferred effort, explicit fields taking precedence; `cachedContent`; `structuredOutputs` default `true`; safetySettings category/`threshold` or one `threshold` across hate/dangerous/harassment/sexual categories; `audioTimestamp`; labels; `mediaResolution`; imageConfig aspectRatio/imageSize/personGeneration/prominentPeople/imageOutputOptions; retrievalConfig latLng under toolConfig; `serviceTier` `standard`/`flex`/`priority`. Vertex-only `streamFunctionCallArguments`/`sharedRequestType`/`requestType` warn and are ignored.

[Fact] PartOptions: `thoughtSignature` for replayed calls/text/files; `thought` for assistant reasoning files; `serverToolCallId`/`serverToolType` for `toolCall`/`toolResponse` replay. Function `strict` makes mode `VALIDATED` if any function requests it.

[Fact] Embeddings: `outputDimensionality`/`taskType`/`content`, one extra-part list or `null` per input, matching lengths. Images: `imageConfig`/`googleSearch`, other `google` fields pass to language options. Speech: `multiSpeakerVoiceConfig`. Transcription: `languageCodes`/`customVocabulary`/`wordTimestamp`/`diarization`/`mode` `SMART` or `VERBATIM`. Video: `personGeneration`/`negativePrompt`/`referenceImages` when frame images exist; otherwise `input_references` supplies references; other fields pass to `parameters`. Files: `displayName`, `pollIntervalMs` 2000, `pollTimeoutMs` 300000. Realtime translationConfig merges into `generationConfig`, other `google` fields into `setup`.

## Provider metadata (provider_metadata["google"])

[Fact] Language results: `promptFeedback`, `groundingMetadata`, `urlContextMetadata`, `safetyRatings`, raw `usageMetadata`, `finishMessage`, `serviceTier` (`null` if absent); copy to custom-`name` key.

[Fact] Parts retain `thoughtSignature`. Server `toolCall` carries `serverToolCallId`/`serverToolType`/signature and maps to dynamic provider-executed `server:<toolType>`. Executable code/results map to `code_execution` or its configured alias.

[Fact] Input total/cache_read/no_cache use promptTokenCount/cachedContentTokenCount/difference. Output text/reasoning/total use candidatesTokenCount/thoughtsTokenCount/sum. Finish `STOP` → `stop` or `tool-calls` for client calls; `MAX_TOKENS` → `length`; safety/recitation/blocklist/prohibited/`SPII`/image-safety → `content-filter`; malformed function call → `error`; otherwise `other`. Blocked prompts without candidates preserve blockReason and finish `content-filter`.

[Fact] Other metadata: positional image empty objects, speech `sampleRate`/`mimeType`, transcription `usage`, video URI entries, file references under google/custom `name` plus `name`/`displayName`/`mimeType`/`sizeBytes`/`state`/`uri`/`createTime`/`updateTime`/`expirationTime`/`sha256Hash`; file-backed batch input IDs/expiry and failed prompt block reasons.

## Known limitations and warnings

- [Fact] Gemini 2.5 drops presence/frequency penalties with warnings; others send them. System messages must precede conversation content.
- [Fact] File bytes become `inlineData`; URLs/references become `fileData`; text is inline plain text. Assistant URL files are unsupported; references must use `google`/`name`. Supported URLs include Files API and YouTube; Gemini except 2.0 also accepts arbitrary HTTPS for 22 media types listed in EXTERNAL_URL_MEDIA_TYPES.
- [Fact] Tool-result files accept bytes/data URLs only, warning and skipping others. Gemini 3+ uses `functionResponse.parts`; older models append `inlineData` plus explanatory text. Errors use `content`; denials use `reason` or default denial text.
- [Fact] Gemini 3+ messages with no signed function call receive sentinel signatures on every call and one warning. If any signed call exists, unsigned calls remain unchanged.
- [Fact] Gemini 3+ mixes function/provider tools with includeServerSideToolInvocations and default VALIDATED mode. Older models keep only provider tools with warnings. Search/enterprise/url/code need Gemini 2+ or `nano-banana`; file search needs 2.5+; unsupported tools are dropped. Vertex RAG warns on Gemini API.
- [Fact] Schema refs must target direct root defs/`definitions` children and are inlined. Recursive function refs use `parametersJsonSchema`; recursive response schemas and mixed-type enums are unsupported. Omit empty root object schemas.
- [Fact] Streams use increasing text/reasoning IDs and generated missing call IDs; `JsonAccumulator` reconstructs `partialArgs`. Gemini has no in-stream error frames; HTTP errors fail before startup, with `retry-after` 429 retryable.
- [Fact] Images reject non-gemini IDs, masks, and n>1. Size warns to use `aspectRatio`; the per-call image maximum is one, including when `with_max_images_per_call` requests a larger value, so core splits multi-image generation into single-image calls. Speech ignores `speed`/`language` with warnings, prefixes `instructions` unless multi-speaker (warn), and returns raw PCM with an explanatory warning when requested.
- [Fact] Video warns for `fps`/`generateAudio`/`webhookUrl`. Reference gs URLs become `gcsUri`; other URLs warn/skip. Resolutions map 1280×720/1920×1080/3840×2160 to `720p`/`1080p`/`4k`, otherwise `WxH`. Whole-second durations use integers. Finished operations return MP4 URLs, adding query `key` only on the configured origin.
- [Fact] Files poll `state`; timeout in `PROCESSING` or `FAILED` returns `ApiCall`. Filename supplies `displayName` when absent.
- [Fact] Batches require one model, put request IDs in `metadata.key`, and reject input files >2 GB. Nonterminal result reads are invalid; completed missing output is `InvalidResponseData`, failed missing output yields empty stream. Classify cancellation by `CANCELLED`/code1, blocked prompts, unsupported file/reasoning/custom/approval content, and invalid responses.
- [Fact] Realtime `SessionUpdate` becomes `setup`; audio append uses PCM at input rate, default 16000. Tool calls emit argument delta/done. `goAway`/`sessionResumptionUpdate`/`toolCallCancellation`/`generationComplete` pass as `Custom`. Unsupported client clear/response-create/response-cancel/truncate/audio-message events return `UnsupportedFunctionality`.
- [Decision] Video exposes only start/status because Veo has only long-running operations; core owns waiting.
- [Decision] Batch image masks/n>1 return `InvalidArgument`, matching single-image preparation.
- [Decision] Batch display names are `ferrin-batch-<id>`; only file-backed startup records input IDs/expiry needed for cleanup.
- [Decision] Core owns secure file downloads; `downloadToolResultFiles` remains outside the provider. [ADR 0023](../04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md) replaces the former Interactions and Live audio exclusions.

## Fixture inventory

Area fixtures replay through FixtureServer; chunks use encoded data events with escaped backslashes/newlines.

| Area | Cases | Coverage |
| --- | --- | --- |
| `generate` | `text`, `tool-call`, `reasoning`, `grounding`, `code-execution`, `server-tool`, `blocked`, `inline-image`, `error-429` | Text/usage/calls/signatures/sources/code/server tools/blocking/images/retry headers |
| `stream` | `text`, `tool-call`, `tool-call-arguments`, `no-args-tool-call`, `reasoning`, `code-execution`, `blocked`, `inline-image` | Contracts/partial and empty arguments/`reasoning`/code/source dedup/blocking/files |
| `embedding` | `single`, `batch` | Requests/count limits |
| `speech` | `generate` | PCM/WAV/raw output/metadata |
| `transcription` | `generate` | Interactions requests/`word_info` |
| `video` | `start`, `status-pending`, `status-done`, `status-error` | Requests/operation mapping |
| `files` | `upload-finalize`, `get-active`, `delete` | Two-stage upload/polling/metadata/`delete` |
| `batch` | `create`, `status-running`, `status-succeeded-inline`, `status-succeeded-file`, `status-failed`, `cancel`, `list`, `results.jsonl` | Requests/state/counts/inline-file results/classification/`cancel`/paging |
| `realtime` | `auth-token` | Token requests/expiry |

[Fact] Fixture regressions snapshot requests, prompts, tool wire shapes, schemas, and events. The ≥20 MB JSONL upload path lacks fixture coverage.

[Pending verification] (PV-031) Handwritten official-schema fixtures need real-response recording.

[Decision] SSE EOF is successful only after an explicit provider terminal response or finish reason (including a Google prompt block). Earlier EOF emits `InvalidResponseData`, closes open parts through the stream driver, and never flushes incomplete tool arguments into executable calls. Source: stream EOF fixture-boundary regressions (2026-09-15); no live API verification.

[Fact] Explicit `thinkingConfig` budget/level fields override generic reasoning; missing fields still inherit its mapping. Regression coverage: `tests/suite/request.rs::explicit_thinking_fields_override_generic_reasoning` (2026-09-15).

[Fact] Assistant file and reasoning-file replay retains the generated `thoughtSignature`; verified by `tests/suite/prompt.rs::generated_files_replay_their_thought_signatures` (2026-09-15).

[Decision] Live API function outputs retain JSON objects directly and wrap other JSON values or plain text in `response.result`, because the wire response must be an object without discarding valid tool output. Regression: `tests/suite/realtime.rs::function_outputs_preserve_every_json_type_and_plain_text` (2026-09-15).

[Decision] Schema conversion maps `true` to an unconstrained object schema (`{}`) and rejects `false` with `UnsupportedFunctionality`, including property, item, union and reference positions; the supported OpenAPI subset cannot express a schema accepting no values. Regression: `tests/suite/unit.rs::boolean_schemas_keep_their_validation_meaning` (2026-09-15).

[Fact] Executable code and its result carry `serverToolType: "code_execution"` metadata and replay as `executableCode`/`codeExecutionResult` parts, including with an application tool alias. Regression: `tests/suite/prompt.rs::generated_code_execution_roundtrips_with_its_result_and_alias` and the code-execution stream fixture (2026-09-15).

[Fact] Returned resumable upload URLs are validated using `url_policy` before file bytes are sent: HTTPS/public addresses by default, resolved addresses pinned, and redirects rejected. Finalization response bodies obey its byte limit. Caller headers are forwarded only to the configured origin or explicit `credentialed_origins`; `x-goog-api-key` is always removed. Local test endpoints require explicit `allow_http().trust_origin(...)`. Sources: `src/files.rs`, `tests/suite/security.rs` (2026-09-15).

[Decision] ADR 0023 supersedes the exclusion of general Interactions, Live streaming transcription and speech translation; implementation and local verification are tracked in [ADR 0023](../04-decisions/2026-09-17-0023-google-interactions-and-live-audio.md).

## Implementation record (2026-09-17): Interactions and Live audio

[Fact] `GoogleInteractionsLanguageModel` uses provider ID `<name>.interactions`; `language_model` and `chat` retain generateContent. Options under `google` and the configured name merge with custom-key precedence: `previousInteractionId`, `store`, `agent`, `agentConfig`, `environment`, `background`, `pollingTimeoutMs` (default 30 minutes), `thinkingLevel`, `thinkingSummaries`, `responseFormat`, `responseModalities`, `mediaResolution`, `serviceTier`, and `systemInstruction`. Prompt system messages win over `systemInstruction` with a warning. Sources: `src/interactions/request.rs`, request snapshot regressions.

[Fact] Interactions converts user text/files, assistant text/files/reasoning/function calls, provider tool replay and tool results. `signature`, `interactionId` and `stepType` metadata survive replay under canonical and custom keys; stored prior assistant history matching `previousInteractionId` is omitted while new tool results remain. `store: false` retains complete history and warns on a prior ID. JSON schemas preserve user property names; response formats use snake-case wire keys. Sources: `src/interactions/prompt.rs`, request and replay regressions.

[Fact] Functions and Google search/code execution/URL context/file search/maps/computer use/retrieval tools have Interactions wire conversion and built-in aliases restore application names. Remote `mcp_server` tools and unknown provider tools warn and are omitted. Unary and streaming results include text, reasoning, files, calls/results, URL/document/maps citations, token counts and service tiers; streaming deduplicates sources. Source: `src/interactions/output.rs`, `sources.rs` and fixture regressions.

[Fact] `start_interaction` returns the initial resource, `get_interaction` reads it, and `cancel_interaction` stops it; resource operations use `GoogleInteractionOptions` headers/cancellation and encode IDs as path segments. `do_generate` polls an in-progress background/agent call once per second up to the configured deadline. `do_stream` starts a background resource then reads incremental `GET /interactions/{id}?stream=true`; an interrupted stream resumes with `last_event_id`, suppressing a duplicated boundary event, with at most two reconnects and the same deadline. A terminal initial POST synthesizes parts immediately. Sources: `src/interactions/background.rs`, lifecycle regressions.

[Decision] Explicit cancellation of a polled/streamed background run attempts a bounded remote cancel and returns cancellation regardless of cleanup outcome. Dropping a stream closes its owned connection without spawning cleanup work; retain the ID via `start_interaction` and use `cancel_interaction` when remote cancellation after drop is required. Missing resume IDs, malformed executable calls, incompatible deltas and premature EOF fail closed; terminal events finish immediately even when the connection remains open. Sources: lifecycle and stream-boundary regressions.

[Fact] With `realtime`, Live transcription accepts `audio/pcm` or `pcm16`, signed 16-bit mono PCM at 16 kHz, for model IDs ending in `-live`. Translation accepts the same input and returns `audio/pcm` at 24 kHz, auto-detects the source language, requires a target language, and accepts `echoTargetLanguage`. Audio submission waits for `setupComplete`; output retains usage and custom-key metadata. Connections use URL policy validation, DNS pinning and bounded frames; cancellation and drop release owned resources. Sources: `src/live_audio`, `src/transcription/live.rs`, `src/speech_translation`, local WebSocket regressions.

[Decision] Audio completion follows the reference adapter's protocol: transcription permits a one-second quiet grace after input EOF; translation uses one second of detected PCM silence (threshold 128) or turn completion plus grace. No detached background task keeps a dropped stream alive. These boundaries are covered by local controlled WebSocket tests with paused time; they do not establish official-provider compatibility. PV-031 remains open for real response recordings.
