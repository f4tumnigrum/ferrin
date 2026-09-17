# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Explicit externally authenticated configuration for dedicated provider adapters,
  allowing Azure credentials without OpenAI key resolution (ADR 0025).

- Four Responses API recordings from a third-party proxy using `gpt-5.6-sol`: text, SSE, function calling and strict structured output, with offline request/output/usage comparisons and stream contract checks. Seven Responses live tests passed; official OpenAI and other provider fixtures remain unverified (PV-031).

### Changed

- **Breaking:** `OpenAiConfig` adds `authentication`; use its constructors when
  migrating struct literals.

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Ignore unknown provider-option keys as the reference object schemas do, while
  continuing to validate known option fields.
- Preserve supplied function and structured-output schemas after compatibility
  normalization; send function strict flags only when supplied, independently of
  `strictJsonSchema`, and keep namespace entries in declaration order.
- Match the reference schemas for all thirteen existing provider tools, including
  input/output validation, defaults, object parsing and request arguments.
- Normalize function output schemas and JSON-encode scalar results for functions
  that declare them; preserve allowed-tool aliases and model-specific async flags.
- Stream multipart file uploads with cancellation and encode file/batch IDs as
  individual path segments; retain file expiry and filename defaults.
- Preserve embedding precision, complete Chat/Completions usage fields, image
  token remainders, and reference speech option precedence.

- Complete hosted program/search/shell mapping, caller bindings, deferred results and replay; expand internal parallel wrappers only for declared functions.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Fixed

- Apply fallible strict schema transforms to Responses and Chat tool inputs and structured outputs.

- Preserve complete raw Chat usage in both generated and streamed results.

- Request base64 image edits and the single-image multipart field for DALL-E models.

- Reject truncated SSE streams that end without an explicit completion signal.

- Preserve opaque dictionary keys when converting provider tool options to wire format.

- Resolve Responses and Chat file references using the configured provider name.

- Encode local shell results with the matching Responses shell output wire type.

- Preserve individual custom tool names and application aliases during Responses replay.

- Send newly executed local tool results when a Responses conversation is configured.
- Verify that Realtime WebSocket configuration debug output redacts its token
  while retaining the authentication subprotocol for the connection.

### Added

- `responses` benchmark: `do_generate` and `do_stream` against the fixture
  server replaying the `text-basic`, `text-basic-stream` and
  `reasoning-stream` fixtures.

## [0.1.0] - 2026-09-14

### Added

- `create_openai` / `OpenAiSettings` / `OpenAiProvider` with lazily loaded
  `OPENAI_API_KEY`, `OPENAI_BASE_URL` fallback, organization and project
  headers, a provider name override and the `ferrin-openai/<version>`
  user-agent suffix.
- Responses API language model (`OpenAiResponsesLanguageModel`): prompt and
  tool conversion, provider options, reasoning, structured output, provider
  tools, generate and stream mapping, batch request bodies.
- Chat Completions (`OpenAiChatLanguageModel`) and legacy Completions
  (`OpenAiCompletionLanguageModel`) language models.
- Embedding, image (generations and edits), speech and transcription models.
- Files, skills and batch APIs; realtime client secrets, session
  configuration and event mapping (`OpenAiRealtimeModel`,
  `OpenAiRealtimeFactory`).
- Provider tool factories (`OpenAiTools`).
- `realtime` feature: WebSocket streaming transcription and
  `OpenAiSpeechTranslationModel`.
- Fixture-driven test suite with stream contract checks and request
  snapshots.

### Changed

- The stream driver (`StreamMachine`, `drive_stream`, `EarlyChunk`) moved to
  `ferrin_provider_util::stream_driver`; `stream_util` re-exports it and
  keeps the OpenAI error mapping and formatting helpers.
- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.
