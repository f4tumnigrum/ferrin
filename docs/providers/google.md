# Google Generative AI (`ferrin-google`)

**English** | [Chinese](../zh-CN/providers/google.md)

`ferrin-google` is the L3 Google Generative AI/Gemini adapter ([Crates](../01-architecture/02-crates.md)). This guide records the 2026-09-14 implementation; see [implementation guide section 12](../01-architecture/17-provider-implementation-guide.md#12-implementation-record-2026-09-14-ferrin-google).

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
| Transcription | Implemented | Single Interactions `POST /interactions`, `word_info` segments; `-live` IDs invalid. |
| Video | Implemented | predictLongRunning start/status with first/last/reference images, aspect/`resolution`/duration/`seed`; direct generation unsupported. |
| Files | Partial | Two-request resumable upload and polling to `ACTIVE`, metadata/delete; no download. |
| Batches | Implemented | batchGenerateContent inline or JSONL upload at ≥20 MB, status, inline/file JSONL results, cancel/list; text and images share image-to-language conversion. |
| Realtime Live API | Implemented | Temporary auth tokens, WebSocket `setup` and bidirectional event mapping; no `speech translation`. |
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
- [Fact] Images reject non-gemini IDs, masks, and n>1. Size warns to use `aspectRatio`; declared per-call image maximum defaults ten and is configurable. Speech ignores `speed`/`language` with warnings, prefixes `instructions` unless multi-speaker (warn), and returns raw PCM with an explanatory warning when requested.
- [Fact] Video warns for `fps`/`generateAudio`/`webhookUrl`. Reference gs URLs become `gcsUri`; other URLs warn/skip. Resolutions map 1280×720/1920×1080/3840×2160 to `720p`/`1080p`/`4k`, otherwise `WxH`. Whole-second durations use integers. Finished operations return MP4 URLs, adding query `key` only on the configured origin.
- [Fact] Files poll `state`; timeout in `PROCESSING` or `FAILED` returns `ApiCall`. Filename supplies `displayName` when absent.
- [Fact] Batches require one model, put request IDs in `metadata.key`, and reject input files >2 GB. Nonterminal result reads are invalid; completed missing output is `InvalidResponseData`, failed missing output yields empty stream. Classify cancellation by `CANCELLED`/code1, blocked prompts, unsupported file/reasoning/custom/approval content, and invalid responses.
- [Fact] Realtime `SessionUpdate` becomes `setup`; audio append uses PCM at input rate, default 16000. Tool calls emit argument delta/done. `goAway`/`sessionResumptionUpdate`/`toolCallCancellation`/`generationComplete` pass as `Custom`. Unsupported client clear/response-create/response-cancel/truncate/audio-message events return `UnsupportedFunctionality`.
- [Decision] Video exposes only start/status because Veo has only long-running operations; core owns waiting.
- [Decision] Batch image masks/n>1 return `InvalidArgument`, matching single-image preparation.
- [Decision] Batch display names are `ferrin-batch-<id>`; only file-backed startup records input IDs/expiry needed for cleanup.
- [Decision] Exclude Interactions beyond transcription, Live streaming transcription, speech translation, and `downloadToolResultFiles`. Core owns secure downloads; other endpoints need stable schemas and future ADRs.

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

Seventy-eight tests snapshot requests, prompts, tool wire shapes, schemas, and events. The ≥20 MB JSONL upload path lacks fixture coverage.

[Pending verification] (PV-031) Handwritten official-schema fixtures need real-response recording.
