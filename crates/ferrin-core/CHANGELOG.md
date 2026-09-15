# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

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
