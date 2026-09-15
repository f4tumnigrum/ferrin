# OpenAI-compatible endpoints (ferrin-openai-compatible)

**English** | [Chinese](../zh-CN/providers/openai-compatible.md)

This L3 generic adapter serves compatible endpoints directly or as a component for specialized providers with custom names/errors/metadata/request transforms ([Crates](../01-architecture/02-crates.md)). This guide records the 2026-09-13 implementation; see [implementation guide section 11](../01-architecture/17-provider-implementation-guide.md#11-implementation-record-2026-09-13-ferrin-openai-compatible).

IDs are `<name>.<family>`; `name` is required, nonempty, and contains no period. Merge keys in increasing priority: `deprecated` `openai-compatible`, `openaiCompatible`, `name`, camelCase(`name`). Metadata uses the caller's camelCase key when present, otherwise `name`.

## Capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| Chat generation/streaming | Implemented | `OpenAiCompatibleChatLanguageModel`, `POST /chat/completions`; default language family; `src/chat/`. |
| Completions generation/streaming | Implemented | `OpenAiCompatibleCompletionLanguageModel`, `POST /completions`; user/assistant text prompts; `src/completion.rs`. |
| Tools | Implemented | Function tools, `strict` only when supplied, all choices. Provider tools warn and are omitted. |
| Structured output | Implemented | With supports_structured_outputs, send json_schema with schema/`strict`/`name`/description, `strict` default `true` and `name` `response`. Otherwise json_object, warning if a schema was supplied. |
| Reasoning | Implemented | Map `reasoning_content`/`reasoning` and `thinking` array parts; send `reasoning_effort` from options or custom `ReasoningEffort`. |
| Embeddings | Implemented | `POST /embeddings`, float encoding; configurable batching/concurrency. |
| Images | Implemented | JSON generation without files, multipart edits with files; at most ten per call. |
| Citations/files/skills/batches/speech/transcription/reranking/video/realtime | Unavailable | No common compatible API; use `Provider` defaults returning `None`. |
| Responses | Unavailable | Use `ferrin-openai` `base_url`/`name` per [ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md). |

## Settings and environment variables

[Fact] `create_openai_compatible(OpenAiCompatibleSettings)`, `src/lib.rs`; only `name`/`base_url` are required:

| Setting | Behavior |
| --- | --- |
| `name` | ID/options prefix; blank or period-containing names are invalid. |
| `base_url` | Required; trim slashes, append endpoint paths and query parameters. |
| `api_key` | Bearer authorization. |
| `api_key_env` | Read each request when explicit key is absent; missing variable omits auth without error for local endpoints. |
| `headers` / call `headers` | Call overrides; append `ferrin-openai-compatible/<version>` User-Agent. |
| `query_params` | Append to every URL, such as `api-version`. |
| `include_usage` | Send stream_options.`include_usage` for Chat/Completions; default `false` because some endpoints reject it. |
| `supports_structured_outputs` | Enables json_schema response format; default `false`. |
| `supported_urls` | Chat file URL patterns, none by default. |
| `error_structure` | Extract error messages/retryability, default OpenAI error message/type/code shape. |
| `metadata_extractor` | Extra Chat metadata from complete responses or individual chunks. |
| `transform_request_body` | Chat request transform before sending. |
| `convert_usage` | Custom Chat usage mapping. |
| `max_embeddings_per_call` / `supports_parallel_calls` | Defaults 2048/`true`. |
| `transport` / `id_generator` | Shared `reqwest`/random fallback tool IDs. |

## Provider options

[Fact] Chat: `user`/`reasoningEffort`/`textVerbosity` (wire `verbosity`)/`strictJsonSchema`. Unconsumed named/camelCase fields pass through; unknown shared-key fields do not. Invalid option shapes return `InvalidArgument`; `src/chat/options.rs`.

[Fact] Completions: `echo`/`logitBias` (`logit_bias`)/`suffix`/`user` plus named extras. Embeddings: `dimensions`/`user`. Images pass all named/camelCase fields into JSON or multipart, omitting `null` form values.

[Fact] Message/part openaiCompatible fields spread into system/user/assistant/tool/text/file/call/result wire objects. Tool `thoughtSignature` from named keys, falling back to `google`, becomes `extra_content.google.thought_signature`.

## Provider metadata

[Fact] Chat reports accepted/rejected prediction tokens, empty when absent, plus extractor metadata; calls preserve Google thought signatures. Embeddings pass `providerMetadata` through. Completions/images have no metadata.

[Fact] Chat `usage` maps prompt total/cache/read/uncached and completion total/reasoning/text, using saturating subtraction; `raw` retains the original. Missing `usage` is empty. Completions map totals, embeddings use prompt_tokens, and images use input/output/`total_tokens`.

## Known limitations and warnings

- [Fact] Chat/Completions warn on `topK`; Completions ignores `tools`/choice/nontext output; images ignore `aspectRatio` (suggest `size`) and `seed`.
- [Fact] Chat image/video accept bytes as detected data URLs or ordinary URLs. Audio accepts only `wav`/`mp3`/mpeg bytes. PDF accepts bytes with default document.pdf filename. Text bytes decode UTF-8; text URLs send their string. References/inline text data/other types are unsupported; assistant files warn and are omitted.
- [Fact] Completions accepts only an initial system prefix, rejects later systems and all tool content, and always includes newline-user `stop`.
- [Fact] Tool text/errors remain strings; denials use `reason`/default text; JSON/errors/content serialize as JSON strings. Skip approval responses.
- [Fact] Buffer tool deltas by `index` until `function.name` arrives, then emit start plus accumulated input; missing names at end emit `InvalidResponseData`. Generate missing IDs.
- [Decision] Errors close open parts and terminate without `Finish`; see guide section 9.
- [Decision] Missing `finish_reason` emits `InvalidResponseData` with `response stream ended without a finish reason`, rather than an `error` `Finish`.
- [Decision] Early `error` frames fail `do_stream` as ApiCall. Three-digit `code` is the status; otherwise infer quota/rate-limit 429, auth 401, `permission` 403, not-found 404, `invalid`/bad-request/context-length 400, `overload` 503, `timeout` 504, default 500. Retry 408/409/429/5xx except `insufficient_quota`. Later errors terminate with the same status. HTTP responses retain status and let ErrorStructure override retryability/message.
- [Fact] Image edit files/masks must be bytes; use `image` or `image[]` form fields. Invalid `b64_json` is `InvalidResponseData`; detect media type from bytes.
- [Fact] Excess embedding counts fail before requests with `TooManyEmbeddingValues`.
- [Fact] Missing `choices` returns `InvalidResponseData` with `response did not contain any choices`.

## Fixture inventory

Area fixtures replay through FixtureServer; streaming cases use `-stream` except error streams, data-only SSE ending in DONE.

| Area | Cases | Coverage |
| --- | --- | --- |
| `chat` | `text-basic`, `reasoning`, `tool-call`, `error-401`, `error-custom`, `text-basic-stream`, `reasoning-stream`, `tool-call-stream`, `no-finish-stream`, `error-early`, `error-late` | Text/cache/prediction/`reasoning`/tools/missing IDs/signatures/custom errors/contracts/delayed names/truncation/early-late failures |
| `completion` | `text-basic`, `text-basic-stream` | Prompt/`stop`/usage/events |
| `embedding` | `basic` | Vectors/usage/metadata passthrough/requests |
| `image` | `generate`, `edit` | Base64/usage/JSON/multipart |

Forty-three tests snapshot requests/prompts/events and cover option keys, inferred errors, headers/query, and all four extension hooks.

[Pending verification] (PV-031) Handwritten official-schema fixtures need real-response recording.

[Decision] Chat and Completion SSE EOF requires an explicit finish reason; otherwise the stream closes open parts and emits `InvalidResponseData`. Chat checks completion before flushing buffered tools, so truncated arguments never become executable calls. Source: `stream_eof` fixture-boundary regressions (2026-09-15); no live API verification.

[Decision] Supported structured output uses the fallible OpenAI strict transform when `strictJsonSchema` is true (default); false preserves the supplied schema, and disabled `supports_structured_outputs` keeps the `json_object` fallback. Function schemas transform only for explicit `strict: true`; absent/false remains unchanged regardless of the response-format flag. Unsupported strict dictionaries fail before HTTP. Source: `strict_schema` regressions (2026-09-15), [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md); no live API verification.
