# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Separate JSON runtime context for generation and agent builders, prepared calls,
  step preparation, approval policies, lifecycle hooks and serialized step results.
- Tool-definition metadata on parsed calls, preliminary and final results, errors,
  provider outcomes and replayed approval outcomes, distinct from provider metadata.

### Changed

- Retain message, instruction, tool context and runtime context overrides across
  generation steps, including streaming and compacted histories (ADR 0021).
  Model, tool-selection and sampling overrides remain local to each step.
- Capture tool context on step results; telemetry requires explicit
  `include_runtime_context` / `include_tools_context` for the respective context. New optional result fields deserialize
  older data, but downstream Rust struct literals must include the added fields.

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Preserve provider routing metadata on local tool outcomes and approval replay,
  including programmatic callers, parallel tool wrappers, automatic denials and
  replayed approval successes, errors and denials.
- Construct description contexts without relying on another crate's feature shape,
  so enabling `ferrin-tool/sandbox` independently does not break the core build.

## [0.1.2] - 2026-09-16

### Fixed

- Apply middleware tool restrictions and normalized tool choices to local execution in both generation loops, with isolated state for concurrent calls and retries.

### Added

- Embedding and image model middleware: `EmbeddingModelMiddleware` /
  `wrap_embedding_model` (with `max_embeddings_per_call`,
  `max_input_bytes_per_call` and `supports_parallel_calls` hooks) and
  `ImageModelMiddleware` / `wrap_image_model` (with `max_images_per_call`),
  mirroring the language model middleware.
- `wrap_provider(provider, ProviderMiddleware)` applies language, embedding and
  image middleware to every model a provider resolves.
- `middleware::builtin::default_embedding_settings(EmbeddingDefaults)` merges
  default headers and provider options into embedding calls.
- `ProviderRegistryBuilder::embedding_model_middleware` and
  `image_model_middleware`; the registry wraps resolved embedding and image
  models like language models.

## [0.1.1] - 2026-09-15

### Fixed

- Embedding telemetry gives each chunk and retry attempt a distinct correlation
  ID and balances failed attempt events, preserving concurrent token metrics.

- Video polling deadlines cover in-flight status calls and retry waits, including
  webhook status checks; cancellation interrupts pending status requests.

- Streaming transcription and speech translation enforce total timeouts during
  establishment and consumption, cancelling provider work on expiry or drop.

- Reasoning extraction preserves unfinished tag text and pairs every reasoning
  block at text/stream boundaries, including empty and unclosed blocks.

- Array output keeps local schema references valid after wrapping nested and
  recursive element schemas.

- Realtime tools honor static and dynamic approval requirements before execution,
  leaving guarded calls for explicit application handling.
- Apply telemetry recording flags to nested step/end payloads, response bodies and error payloads/causes and warning descriptions while preserving error classifications and original application results.

- Preserve delta metadata while smoothing text and reasoning, including buffered transitions and metadata-only deltas.

- Cache successful URL downloads across text-generation steps, retaining identical file bytes when prepare_step switches models.

- Invoke explicit custom downloaders even when the model supports every referenced URL.

- Register cancellation wakeups while tools are pending so cancellation promptly drops execution and ends both generation loops.

- Validate every approved tool context before replay starts and propagate context validation errors throughout tool execution.

- Deduplicate approval responses by approval and tool-call IDs before execution, rejecting conflicting decisions.

- Reject model calls and repaired calls to tools excluded by caller restrictions or the current active tool set.

- Enforce required and named tool choices consistently after generate and stream model calls.

- Clear effective required or named tool choices when step preparation filters out every tool.

### Added

- `generate_text` benchmark (single step, 11- and 51-message histories,
  two-step tool loop, `extract_reasoning` middleware) and `stream_text`
  benchmark (`text_stream`, event stream, `consume`, word-chunked
  `smooth_stream`) over `MockLanguageModel`.

## [0.1.0] - 2026-09-14

### Added

- `Error` (`#[non_exhaustive]`, 128 bytes or less) with `ErrorKind`,
  `RetryReason`, boxed payloads for provider, download, tool input,
  structured output and registry failures, `NoVideoGenerated`, `Stream`
  and `ToolChoiceNotSatisfied` variants.
- `generate_text`: multi-step tool loop with stop conditions (`step_count`,
  `has_tool_call`), tool execution with per-tool timeouts and concurrency
  limits, tool call repair and input refinement, approval requests and
  signed approval responses, `prepare_step`, `Include` options,
  `GenerateTextResult`, `StepResult`, `StepContent`, response message
  assembly and structured output parsing.
- `stream_text`: staged streaming pipeline (`StreamEvent`, part id
  remapping, tool execution, stream-level retries with `RetryAttempt`
  events, stop gating, user transforms with `TransformContext::stop`),
  `StreamTextResult`, completion futures and partial output streams.
- `Output::{text, object, array, choice, json}` structured output
  strategies.
- `agent`: `Agent` trait, `AgentCall`, `AgentStreamCall`, `ToolLoopAgent`
  with `prepare_call`, default `step_count(20)` stop condition and the
  `ferrin-agent/tool-loop` user-agent suffix.
- `middleware`: `LanguageModelMiddleware`, `wrap_language_model`, built-in
  `default_settings`, `extract_reasoning`, `simulate_streaming`,
  `extract_json`, `add_tool_input_examples`.
- `registry`: `ProviderRegistry`, `create_provider_registry`,
  `custom_provider`, explicit process-wide default registry, resolution of
  every model kind including realtime models.
- `retry::RetryPolicy` (backoff, jitter, `Retry-After` handling),
  `timeout::Timeout` with total/step/first-chunk/chunk/tool scopes,
  `hooks::Hooks`, `telemetry::{Telemetry, TelemetryOptions}` with `tracing`
  spans (`ferrin.generate_text`, `ferrin.stream_text`, `ferrin.model_call`,
  `ferrin.tool_call`, `ferrin.modality`), `clock::Clock`, `ids`.
- `prompt`: `CallSettings`, `Instructions`, message standardisation and
  conversion to the specification prompt, URL download policy
  (`DownloadFn`, `DefaultDownloader`).
- Modalities: `embed`, `embed_many` (limit-based chunking, parallel calls),
  `cosine_similarity`, `generate_image`, `edit_image`, `generate_speech`,
  `transcribe`, `stream_transcribe`, `rerank`, `generate_video` (feature
  `video`; synchronous, polling and webhook flows), `upload_file`,
  `get_file_metadata`, `download_file`, `delete_file`, `upload_skill`,
  `start_batch`, `get_batch_status`, `get_batch_results`, `cancel_batch`,
  `list_batches`, `stream_speech_translation`.
- Feature `realtime`: `realtime_session`, `RealtimeSession`,
  `RealtimeHandle`, `realtime_tool_definitions`; local function tools are
  executed inside the session and a single follow-up response is requested
  once every tool call of a response has an output.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.
